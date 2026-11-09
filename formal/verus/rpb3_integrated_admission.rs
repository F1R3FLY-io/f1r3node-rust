use vstd::prelude::*;

mod rpb3_admission_cursor;
mod rpb3_score_transcript;
mod rpb3_map_frame_refinement;
mod rpb3_epathmap_event_refinement;
pub mod rpb3_path_parser_refinement;
mod rpb3_canonical_path_common_product;
mod rpb3_epm_source_accept_refinement;
mod rpb3_epm_nested_source_refinement;
mod rpb3_score_admission_bridge;
mod rpb3_finish_macro_refinement;
mod rpb3_runtime_frame_source_refinement;
mod rpb3_score_arena_source_refinement;
mod rpb3_wire_cursor_source_refinement;
#[cfg(rpb3_live_push)]
mod rpb3_source_observation;
#[cfg(rpb3_live_push)]
mod rpb3_map_eof_indexed_view_identity;
#[cfg(rpb3_live_push)]
mod rpb3_map_push_receipt_source_extraction;
#[cfg(rpb3_live_push)]
mod rpb3_map_push_backing_identity;
#[cfg(rpb3_live_push)]
mod rpb3_map_push_scoped_identity_classes;
#[cfg(rpb3_live_push)]
mod rpb3_map_push_finite_observation;
#[cfg(rpb3_live_push)]
mod rpb3_map_push_selected_subview_extraction;
#[cfg(rpb3_live_push)]
mod rpb3_map_push_live_parent_bounds;
#[cfg(rpb3_live_push)]
mod rpb3_map_push_fresh_child_extraction;
#[cfg(rpb3_live_push)]
mod rpb3_map_push_live_child_projection;
#[cfg(rpb3_live_push)]
mod rpb3_map_push_checked_receipt_projection;
mod rpb3_generic_message_eof_source_refinement;
mod rpb3_message_driver_source_integration;
mod rpb3_canonical_path_driver_source_refinement;
mod rpb3_canonical_path_driver_product_integration;
mod rpb3_canonical_path_persistent_state_source;
mod rpb3_canonical_path_historical_checkpoint_source;
mod rpb3_canonical_path_forward_capacity_source;
mod rpb3_canonical_path_owner_health_source;
mod rpb3_live_canonical_path_source_refinement;
mod rpb3_live_canonical_path_trace_refinement;
#[cfg(rpb3_live_push)]
mod rpb3_live_canonical_path_descendant_action_lift;
#[cfg(rpb3_live_push)]
mod rpb3_live_canonical_path_score_action_lift;
#[cfg(rpb3_live_push)]
mod rpb3_live_canonical_path_pop_action_lift;
#[cfg(rpb3_live_push)]
mod rpb3_live_canonical_path_map_finish_action_lift;
#[cfg(rpb3_live_push)]
mod rpb3_live_canonical_path_push_action_lift;
mod rpb3_canonical_path_root_source_refinement;
mod rpb3_live_canonical_path_terminal_refinement;
mod rpb3_live_canonical_path_root_terminal_trace;
mod rpb3_canonical_path_terminal_driver_observation;
mod rpb3_canonical_path_root_complete_source_correspondence;
mod rpb3_epm_nested_accept_checked_stats_refinement;
mod rpb3_map_key_source_correspondence;
mod rpb3_map_key_full_source_composition;
mod rpb3_epathmap_terminal_source_observation;
#[cfg(rpb3_live_push)]
mod rpb3_epathmap_end_source_selector;
mod rpb3_epm_push_source_action_refinement;
mod rpb3_epm_push_transport_source_refinement;
mod rpb3_epm_restore_run_expansion;
mod rpb3_epm_batch_cursor_composition;
mod rpb3_epm_batch_cursor_source_extraction;
mod rpb3_epm_active_line_work_source;
mod rpb3_act_source_product_integration;
mod rpb3_epm_unwind_event_lift;
mod rpb3_epm_unwind_product_lift;
#[cfg(rpb3_live_push)]
mod rpb3_epm_push_live_action_refinement;
#[cfg(rpb3_live_push)]
mod rpb3_epm_loop_continuation_source;
#[cfg(rpb3_live_push)]
mod rpb3_epm_loop_observation_bridge;
#[cfg(all(rpb3_live_push, rpb3_terminal))]
mod rpb3_integrated_source_trace_composition;
mod rpb3_epathmap_terminal_nested_return_composition;
#[cfg(rpb3_live_push)]
mod rpb3_integrated_dispatcher;

use {
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_nested_source_refinement as epm_nested_source,
    rpb3_epm_source_accept_refinement as epm_source_accept, rpb3_finish_macro_refinement as finish,
    rpb3_map_frame_refinement as map, rpb3_runtime_frame_source_refinement as runtime_source,
    rpb3_score_admission_bridge as score_bridge,
    rpb3_score_arena_source_refinement as arena_source,
    rpb3_wire_cursor_source_refinement as wire_source,
};

verus! {

pub enum SourceSidecar {
    Map { state: map::IntegratedAdmissionState },
    Epm { state: epm::LinkedEpmState },
}

pub struct IntegratedScoreState {
    pub ghost bridge: score_bridge::ScoreAdmissionBridge,
    pub ghost sidecar: SourceSidecar,
}

pub open spec fn integrated_score_wf(state: IntegratedScoreState) -> bool {
    score_bridge::bridge_well_formed(state.bridge)
        && admission::admission_state_wf(state.bridge.admission)
        && match state.sidecar {
            SourceSidecar::Map { state: map_state } => {
                &&& map::integrated_admission_wf(map_state)
                &&& map_state.admission == state.bridge.admission
                &&& map_state.transcript == state.bridge.transcript
            },
            SourceSidecar::Epm { state: epm_state } => {
                &&& epm::linked_indexed_sources(epm_state)
                &&& epm_state.admission == state.bridge.admission
                &&& epm::frame_layout_wf(epm_state.epm)
            },
        }
}

pub open spec fn map_root_initial(
    root: admission::AdmissionFrameState,
) -> Option<IntegratedScoreState> {
    if !admission::admission_root_initial(root) {
        None
    } else {
        match map::integrated_admission_initial(root) {
            None => None,
            Some(map_state) => Some(IntegratedScoreState {
                bridge: score_bridge::bridge_initial(root),
                sidecar: SourceSidecar::Map { state: map_state },
            }),
        }
    }
}

pub proof fn map_root_initial_exact(root: admission::AdmissionFrameState)
    requires admission::admission_root_initial(root),
    ensures
        map_root_initial(root).is_some(),
        integrated_score_wf(map_root_initial(root).unwrap()),
{
    map::integrated_initial_is_well_formed(root);
    score_bridge::bridge_initial_exact(root);
    admission::admission_initial_wf(root);
}

pub enum IntegratedSourceAction {
    Map { action: map::IntegratedAdmissionAction },
    EpmHandoff {
        indexed_regions: Seq<epm::IndexedRegionImage>,
        backing: epm::IndexedBackingImage,
        plan: Option<epm::ActPlanImage>,
        validated: Option<epm::ValidatedActPlanImage>,
    },
    EpmContinue { event: epm::EpmEvent },
    EpmPush {
        event: epm::EpmEvent,
        action: epm::EpmAction,
        child: admission::AdmissionFrameState,
    },
    EpmScore,
    EpmChildReturn {
        summary: epm::ChildSummary,
        admission_summary: admission::AdmissionReturnedKind,
        nested_before: Option<epm::EpmFrame>,
        nested_after: Option<epm::EpmFrame>,
    },
    EpmComplete { action: epm::EpmAction },
}

pub open spec fn integrated_epm_handoff_step(
    before: IntegratedScoreState,
    after: IntegratedScoreState,
    indexed_regions: Seq<epm::IndexedRegionImage>,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
) -> bool {
    match (before.sidecar, after.sidecar) {
        (SourceSidecar::Map { state: prior }, SourceSidecar::Epm { state: next }) => {
            &&& prior.admission == before.bridge.admission
            &&& prior.transcript == before.bridge.transcript
            &&& epm::linked_map_epm_handoff(
                prior.admission, indexed_regions, backing, plan, validated,
            ) == Some(next)
            &&& score_bridge::bridge_same(before.bridge, after.bridge)
        },
        _ => false,
    }
}

pub open spec fn integrated_map_step(
    before: IntegratedScoreState,
    after: IntegratedScoreState,
    action: map::IntegratedAdmissionAction,
) -> bool {
    match (before.sidecar, after.sidecar) {
        (SourceSidecar::Map { state: prior }, SourceSidecar::Map { state: next }) => {
            &&& prior.admission == before.bridge.admission
            &&& prior.transcript == before.bridge.transcript
            &&& match map::integrated_admission_driver_step(prior, action) {
                None => false,
                Some(outcome) => map::integrated_nonterminal_state(outcome) == Some(next),
            }
            &&& after.bridge.admission == next.admission
            &&& after.bridge.transcript == next.transcript
        },
        _ => false,
    }
}

pub open spec fn integrated_epm_continue_step(
    before: IntegratedScoreState,
    after: IntegratedScoreState,
    event: epm::EpmEvent,
) -> bool {
    match (before.sidecar, after.sidecar) {
        (SourceSidecar::Epm { state: prior }, SourceSidecar::Epm { state: next }) => {
            &&& prior.admission == before.bridge.admission
            &&& epm::linked_epm_continue(prior, next, event)
            &&& next.admission == after.bridge.admission
            &&& after.bridge.transcript == before.bridge.transcript
        },
        _ => false,
    }
}

pub open spec fn integrated_epm_push_step(
    before: IntegratedScoreState,
    after: IntegratedScoreState,
    event: epm::EpmEvent,
    action: epm::EpmAction,
    child: admission::AdmissionFrameState,
) -> bool {
    match (before.sidecar, after.sidecar) {
        (SourceSidecar::Epm { state: prior }, SourceSidecar::Epm { state: next }) => {
            &&& prior.admission == before.bridge.admission
            &&& epm::linked_epm_push(prior, next, event, action, child)
            &&& next.admission == after.bridge.admission
            &&& after.bridge.transcript == before.bridge.transcript
        },
        _ => false,
    }
}

pub open spec fn integrated_epm_score_step(
    before: IntegratedScoreState,
    after: IntegratedScoreState,
) -> bool {
    match (before.sidecar, after.sidecar) {
        (SourceSidecar::Epm { state: prior }, SourceSidecar::Epm { state: next }) => {
            &&& prior.admission == before.bridge.admission
            &&& score_bridge::bridge_score_source_step(before.bridge, after.bridge)
            &&& next.admission == after.bridge.admission
            &&& next.epm == prior.epm
            &&& next.indexed_regions == prior.indexed_regions
            &&& epm::linked_indexed_sources(next)
        },
        _ => false,
    }
}

pub open spec fn integrated_epm_child_return_step(
    before: IntegratedScoreState,
    after: IntegratedScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_before: Option<epm::EpmFrame>,
    nested_after: Option<epm::EpmFrame>,
) -> bool {
    match (before.sidecar, after.sidecar) {
        (SourceSidecar::Epm { state: prior }, SourceSidecar::Epm { state: next }) => {
            &&& prior.admission == before.bridge.admission
            &&& epm::linked_epm_child_return(
                prior, next, summary, admission_summary,
                nested_before, nested_after,
            )
            &&& next.admission == after.bridge.admission
            &&& after.bridge.transcript == before.bridge.transcript
        },
        _ => false,
    }
}

pub open spec fn integrated_epm_complete_step(
    before: IntegratedScoreState,
    after: IntegratedScoreState,
    action: epm::EpmAction,
) -> bool {
    match (before.sidecar, after.sidecar) {
        (SourceSidecar::Epm { state: prior }, SourceSidecar::Epm { state: next }) => {
            &&& prior.admission == before.bridge.admission
            &&& epm::linked_epm_complete(prior, next, action)
            &&& next.admission == after.bridge.admission
            &&& after.bridge.transcript == before.bridge.transcript
        },
        _ => false,
    }
}

pub open spec fn integrated_source_step(
    before: IntegratedScoreState,
    after: IntegratedScoreState,
    action: IntegratedSourceAction,
) -> bool {
    integrated_score_wf(before) && match action {
        IntegratedSourceAction::Map { action } =>
            integrated_map_step(before, after, action),
        IntegratedSourceAction::EpmHandoff {
            indexed_regions, backing, plan, validated,
        } => integrated_epm_handoff_step(
            before, after, indexed_regions, backing, plan, validated,
        ),
        IntegratedSourceAction::EpmContinue { event } =>
            integrated_epm_continue_step(before, after, event),
        IntegratedSourceAction::EpmPush { event, action, child } =>
            integrated_epm_push_step(before, after, event, action, child),
        IntegratedSourceAction::EpmScore =>
            integrated_epm_score_step(before, after),
        IntegratedSourceAction::EpmChildReturn {
            summary, admission_summary, nested_before, nested_after,
        } => integrated_epm_child_return_step(
            before, after, summary, admission_summary, nested_before, nested_after,
        ),
        IntegratedSourceAction::EpmComplete { action } =>
            integrated_epm_complete_step(before, after, action),
    }
}

pub open spec fn map_root_trace(
    root: admission::AdmissionFrameState,
    states: Seq<IntegratedScoreState>,
    actions: Seq<IntegratedSourceAction>,
) -> bool {
    &&& admission::admission_root_initial(root)
    &&& states.len() == actions.len() + 1
    &&& map_root_initial(root) == Some(states[0])
    &&& forall|i: int| 0 <= i < actions.len()
        ==> integrated_source_step(states[i], states[i + 1], actions[i])
}

pub open spec fn integrated_reachable(state: IntegratedScoreState) -> bool {
    exists|root: admission::AdmissionFrameState,
        states: Seq<IntegratedScoreState>, actions: Seq<IntegratedSourceAction>|
        map_root_trace(root, states, actions)
            && states[states.len() - 1] == state
}

pub proof fn unchanged_admission_arena_preserves_exact(
    before: score_bridge::ScoreAdmissionBridge,
    after: score_bridge::ScoreAdmissionBridge,
)
    requires
        score_bridge::bridge_well_formed(before),
        after.admission.arena == before.admission.arena,
        after.transcript == before.transcript,
    ensures score_bridge::bridge_well_formed(after),
{
    assert forall|i: int| 0 <= i < after.admission.arena.len()
        implies after.admission.arena[i].encoded_len
            == after.transcript.entries[i].encoded_len
            && after.admission.arena[i].transcript_bytes
                == after.transcript.entries[i].transcript by {}
}

pub proof fn integrated_source_step_preserves_exact(
    before: IntegratedScoreState,
    after: IntegratedScoreState,
    action: IntegratedSourceAction,
)
    requires integrated_source_step(before, after, action),
    ensures integrated_score_wf(after),
{
    match action {
        IntegratedSourceAction::Map { action: map_action } => {
            match (before.sidecar, after.sidecar) {
                (SourceSidecar::Map { state: prior }, SourceSidecar::Map { state: next }) => {
                    map::integrated_driver_preserves_wf(prior, map_action);
                    assert(map::integrated_admission_wf(next));
                    assert(score_bridge::bridge_well_formed(after.bridge));
                    assert(admission::admission_state_wf(after.bridge.admission));
                },
                _ => assert(false),
            }
        },
        IntegratedSourceAction::EpmHandoff { .. } => {
            match (before.sidecar, after.sidecar) {
                (SourceSidecar::Map { .. }, SourceSidecar::Epm { state: next }) => {
                    assert(score_bridge::bridge_same(before.bridge, after.bridge));
                    assert(next.admission == before.bridge.admission);
                    assert(epm::linked_indexed_sources(next));
                    assert(epm::frame_layout_wf(next.epm));
                },
                _ => assert(false),
            }
        },
        IntegratedSourceAction::EpmContinue { event } => {
            match (before.sidecar, after.sidecar) {
                (SourceSidecar::Epm { state: prior }, SourceSidecar::Epm { state: next }) => {
                    epm::linked_epm_continue_preserves_source_and_progress(
                        prior, next, event,
                    );
                    assert(after.bridge.admission.arena == before.bridge.admission.arena);
                    unchanged_admission_arena_preserves_exact(before.bridge, after.bridge);
                    assert(epm::linked_indexed_sources(next));
                    assert(epm::frame_layout_wf(next.epm));
                },
                _ => assert(false),
            }
        },
        IntegratedSourceAction::EpmPush { event, action, child } => {
            match (before.sidecar, after.sidecar) {
                (SourceSidecar::Epm { state: prior }, SourceSidecar::Epm { state: next }) => {
                    epm::linked_epm_push_preserves_parent_progress(
                        prior, next, event, action, child,
                    );
                    assert(after.bridge.admission.arena == before.bridge.admission.arena);
                    unchanged_admission_arena_preserves_exact(before.bridge, after.bridge);
                    assert(epm::linked_indexed_sources(next));
                    assert(epm::frame_layout_wf(next.epm));
                },
                _ => assert(false),
            }
        },
        IntegratedSourceAction::EpmScore => {
            score_bridge::bridge_score_source_step_preserves_exact(
                before.bridge, after.bridge,
            );
        },
        IntegratedSourceAction::EpmChildReturn { .. } => {
            match (before.sidecar, after.sidecar) {
                (SourceSidecar::Epm { .. }, SourceSidecar::Epm { state: next }) => {
                    assert(after.bridge.admission.arena == before.bridge.admission.arena);
                    unchanged_admission_arena_preserves_exact(before.bridge, after.bridge);
                    assert(epm::linked_indexed_sources(next));
                    assert(epm::frame_layout_wf(next.epm));
                },
                _ => assert(false),
            }
        },
        IntegratedSourceAction::EpmComplete { .. } => {
            match (before.sidecar, after.sidecar) {
                (SourceSidecar::Epm { .. }, SourceSidecar::Epm { state: next }) => {
                    assert(after.bridge.admission.arena == before.bridge.admission.arena);
                    unchanged_admission_arena_preserves_exact(before.bridge, after.bridge);
                    assert(epm::linked_indexed_sources(next));
                    assert(epm::frame_layout_wf(next.epm));
                },
                _ => assert(false),
            }
        },
    }
}

pub proof fn map_root_trace_prefix_exact(
    root: admission::AdmissionFrameState,
    states: Seq<IntegratedScoreState>,
    actions: Seq<IntegratedSourceAction>,
    index: int,
)
    requires map_root_trace(root, states, actions), 0 <= index < states.len(),
    ensures integrated_score_wf(states[index]),
    decreases index,
{
    if index == 0 {
        map_root_initial_exact(root);
    } else {
        map_root_trace_prefix_exact(root, states, actions, index - 1);
        integrated_source_step_preserves_exact(
            states[index - 1], states[index], actions[index - 1],
        );
    }
}

pub open spec fn integrated_root_witness(
    root: admission::AdmissionFrameState, state: IntegratedScoreState,
) -> bool {
    exists|states: Seq<IntegratedScoreState>, actions: Seq<IntegratedSourceAction>|
        #[trigger] map_root_trace(root, states, actions)
            && states[states.len() - 1] == state
}

pub proof fn integrated_reachable_exact(state: IntegratedScoreState)
    requires integrated_reachable(state),
    ensures integrated_score_wf(state),
{
    let (root, states, actions) = choose|root: admission::AdmissionFrameState,
        states: Seq<IntegratedScoreState>, actions: Seq<IntegratedSourceAction>|
        map_root_trace(root, states, actions)
            && states[states.len() - 1] == state;
    assert(map_root_trace(root, states, actions));
    assert(states.len() == actions.len() + 1);
    assert(states.len() > 0);
    map_root_trace_prefix_exact(root, states, actions, states.len() - 1);
    assert(states[states.len() - 1] == state);
}

pub struct ProductScoreState {
    pub ghost bridge: score_bridge::ScoreAdmissionBridge,
    pub ghost map: map::IntegratedAdmissionState,
    pub ghost epm_frames: Seq<Option<epm::EpmFrame>>,
    pub ghost indexed_regions: Seq<epm::IndexedRegionImage>,
    pub ghost accepted_epm_sources: Seq<Option<epm::LinkedEpmState>>,
}

pub open spec fn product_epm_source_receipt_matches(
    frame: admission::AdmissionFrameState,
    image: epm::IndexedRegionImage,
    source: epm::LinkedEpmState,
) -> bool {
    match epm_source_accept::epm_source_finish_accept(source) {
        None => false,
        Some(accepted) => {
            &&& accepted.frames.len() > 0
            &&& accepted.frames.last().returns.len() > 0
            &&& frame.family == admission::AdmissionFamily::Other
            &&& frame.program_type_id == 31
            &&& frame.body_id == accepted.frames.last().body_id
            &&& frame.physical_start == accepted.frames.last().physical_start
            &&& frame.physical_source == accepted.frames.last().physical_source
            &&& frame.region_start == accepted.frames.last().region_start
            &&& frame.region_end == accepted.frames.last().region_end
            &&& frame.source_bytes == accepted.frames.last().source_bytes
            &&& source.indexed_regions.len() > 1
            &&& image == source.indexed_regions[
                source.indexed_regions.len() - 2
            ]
            &&& frame.returns.len() > 0
            &&& frame.returns.last() == accepted.frames.last().returns.last()
            &&& frame.returns.last().body_bytes == source.epm.snapshot.bytes
        },
    }
}

pub open spec fn product_epm_source_receipts_wf(state: ProductScoreState) -> bool {
    state.accepted_epm_sources.len() == state.bridge.admission.frames.len()
        && state.indexed_regions.len() == state.bridge.admission.frames.len()
        && forall|i: int| 0 <= i < state.accepted_epm_sources.len()
            ==> match #[trigger] state.accepted_epm_sources[i] {
                None => true,
                Some(source) => product_epm_source_receipt_matches(
                    state.bridge.admission.frames[i], state.indexed_regions[i], source,
                ),
            }
}

pub open spec fn product_epm_receipts_carried(
    before: ProductScoreState, after: ProductScoreState,
) -> Seq<Option<epm::LinkedEpmState>> {
    Seq::new(after.bridge.admission.frames.len(), |i: int|
        if i < before.bridge.admission.frames.len()
            && i < before.accepted_epm_sources.len()
            && before.bridge.admission.frames[i].body_id
                == after.bridge.admission.frames[i].body_id
            && before.bridge.admission.frames[i].family
                == after.bridge.admission.frames[i].family
            && before.bridge.admission.frames[i].program_type_id
                == after.bridge.admission.frames[i].program_type_id
            && before.bridge.admission.frames[i].physical_start
                == after.bridge.admission.frames[i].physical_start
            && before.bridge.admission.frames[i].physical_source
                == after.bridge.admission.frames[i].physical_source
            && before.bridge.admission.frames[i].region_start
                == after.bridge.admission.frames[i].region_start
            && before.bridge.admission.frames[i].region_end
                == after.bridge.admission.frames[i].region_end
            && before.bridge.admission.frames[i].source_bytes
                == after.bridge.admission.frames[i].source_bytes
            && before.bridge.admission.frames[i].returns
                == after.bridge.admission.frames[i].returns
            && i < before.indexed_regions.len()
            && i < after.indexed_regions.len()
            && before.indexed_regions[i] == after.indexed_regions[i]
        {
            before.accepted_epm_sources[i]
        } else { None })
}

pub proof fn product_epm_receipts_carried_preserves(
    before: ProductScoreState, after: ProductScoreState,
)
    requires
        product_epm_source_receipts_wf(before),
        after.accepted_epm_sources == product_epm_receipts_carried(before, after),
        after.indexed_regions.len() == after.bridge.admission.frames.len(),
    ensures product_epm_source_receipts_wf(after),
{
    assert forall|i: int| 0 <= i < after.accepted_epm_sources.len()
        implies match #[trigger] after.accepted_epm_sources[i] {
            None => true,
            Some(source) => product_epm_source_receipt_matches(
                after.bridge.admission.frames[i], after.indexed_regions[i], source,
            ),
        } by {
        assert(after.accepted_epm_sources[i]
            == product_epm_receipts_carried(before, after)[i]);
        if after.accepted_epm_sources[i].is_some() {
            assert(before.bridge.admission.frames[i].body_id
                == after.bridge.admission.frames[i].body_id);
            assert(before.bridge.admission.frames[i].family
                == after.bridge.admission.frames[i].family);
            assert(before.bridge.admission.frames[i].program_type_id
                == after.bridge.admission.frames[i].program_type_id);
            assert(before.bridge.admission.frames[i].physical_start
                == after.bridge.admission.frames[i].physical_start);
            assert(before.bridge.admission.frames[i].physical_source
                == after.bridge.admission.frames[i].physical_source);
            assert(before.bridge.admission.frames[i].region_start
                == after.bridge.admission.frames[i].region_start);
            assert(before.bridge.admission.frames[i].region_end
                == after.bridge.admission.frames[i].region_end);
            assert(before.bridge.admission.frames[i].source_bytes
                == after.bridge.admission.frames[i].source_bytes);
            assert(before.bridge.admission.frames[i].returns
                == after.bridge.admission.frames[i].returns);
            assert(before.indexed_regions[i] == after.indexed_regions[i]);
        }
    }
}

pub open spec fn product_score_wf(state: ProductScoreState) -> bool {
    &&& score_bridge::bridge_well_formed(state.bridge)
    &&& map::integrated_admission_wf(state.map)
    &&& state.map.admission == state.bridge.admission
    &&& state.map.transcript == state.bridge.transcript
    &&& state.epm_frames.len() == state.bridge.admission.frames.len()
    &&& state.indexed_regions.len() == state.bridge.admission.frames.len()
    &&& product_epm_source_receipts_wf(state)
    &&& product_frame_source_anchors_match(state)
    &&& forall|i: int| 0 <= i < state.epm_frames.len() ==> {
        &&& epm::indexed_image_wf(state.indexed_regions[i])
        &&& state.indexed_regions[i].bytes
            == state.bridge.admission.frames[i].source_bytes
        &&& match state.epm_frames[i] {
            None => true,
            Some(frame) => {
                &&& state.bridge.admission.frames[i].frame_kind == 3
                &&& frame.snapshot == state.indexed_regions[i]
                &&& epm::frame_layout_wf(frame)
            },
        }
    }
}

pub proof fn product_score_wf_intro(state: ProductScoreState)
    requires
        score_bridge::bridge_well_formed(state.bridge),
        map::integrated_admission_wf(state.map),
        state.map.admission == state.bridge.admission,
        state.map.transcript == state.bridge.transcript,
        state.epm_frames.len() == state.bridge.admission.frames.len(),
        state.indexed_regions.len() == state.bridge.admission.frames.len(),
        product_epm_source_receipts_wf(state),
        product_frame_source_anchors_match(state),
        forall|i: int| 0 <= i < state.epm_frames.len() ==> {
            &&& epm::indexed_image_wf(state.indexed_regions[i])
            &&& state.indexed_regions[i].bytes
                == state.bridge.admission.frames[i].source_bytes
            &&& match state.epm_frames[i] {
                None => true,
                Some(frame) => {
                    &&& state.bridge.admission.frames[i].frame_kind == 3
                    &&& frame.snapshot == state.indexed_regions[i]
                    &&& epm::frame_layout_wf(frame)
                },
            }
        },
    ensures product_score_wf(state),
{
    assert(score_bridge::bridge_well_formed(state.bridge));
    assert(map::integrated_admission_wf(state.map));
    assert(state.map.admission == state.bridge.admission);
    assert(state.map.transcript == state.bridge.transcript);
    assert(state.epm_frames.len() == state.bridge.admission.frames.len());
    assert(state.indexed_regions.len() == state.bridge.admission.frames.len());
    assert(product_epm_source_receipts_wf(state));
    assert(product_frame_source_anchors_match(state));
    assert forall|i: int| 0 <= i < state.epm_frames.len() implies {
        &&& epm::indexed_image_wf(state.indexed_regions[i])
        &&& state.indexed_regions[i].bytes
            == state.bridge.admission.frames[i].source_bytes
        &&& match state.epm_frames[i] {
            None => true,
            Some(frame) => {
                &&& state.bridge.admission.frames[i].frame_kind == 3
                &&& frame.snapshot == state.indexed_regions[i]
                &&& epm::frame_layout_wf(frame)
            },
        }
    } by {};
    assert(product_score_wf(state));
}

pub open spec fn product_root_initial(
    root: admission::AdmissionFrameState,
    backing: epm::IndexedBackingImage,
) -> Option<ProductScoreState> {
    if !admission::admission_root_initial(root) {
        None
    } else {
        match (
            map::integrated_admission_initial(root),
            epm::indexed_backing_project(backing),
        ) {
            (Some(map_state), Some(image)) if epm::indexed_image_wf(image)
                && image.bytes == root.source_bytes
                && root.physical_source == product_indexed_start_anchor(image).source
                && root.physical_start == product_indexed_start_anchor(image).offset => Some(ProductScoreState {
                bridge: score_bridge::bridge_initial(root),
                map: map_state,
                epm_frames: seq![None],
                indexed_regions: seq![image],
                accepted_epm_sources: seq![None],
            }),
            _ => None,
        }
    }
}

#[cfg(rpb3_terminal)]
pub proof fn product_root_finish_claim_exact(
    state: ProductScoreState, claimed: Seq<u8>,
)
    requires product_score_wf(state),
    ensures match map::integrated_admission_driver_step(
        state.map,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        },
    ) {
        Some(map::IntegratedAdmissionOutcome::RootFinished { root_summary }) => {
            let handle = root_summary.score_handle;
            &&& admission::admission_complete_root(state.bridge.admission)
            &&& score_bridge::bridge_root_handle(state.bridge) == Some(handle)
            &&& root_summary.complete
                == state.bridge.admission.arena[handle as int].semantic
            &&& state.bridge.admission.arena[handle as int].transcript_bytes
                == state.bridge.transcript.entries[handle as int].transcript
            &&& score_bridge::bridge_root_first_difference(state.bridge, claimed).is_none()
                == rpb3_score_transcript::claimed_score_matches(
                    state.bridge.transcript, handle, claimed,
                )
        },
        _ => true,
    },
{
    map::integrated_root_finish_returns_derived_summary(state.map);
    if let Some(map::IntegratedAdmissionOutcome::RootFinished { root_summary }) =
        map::integrated_admission_driver_step(
            state.map,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        )
    {
        assert(admission::admission_complete_root(state.map.admission));
        assert(state.map.admission == state.bridge.admission);
        score_bridge::bridge_root_exact(state.bridge);
        score_bridge::bridge_root_first_difference_exact(state.bridge, claimed);
    }
}

pub open spec fn product_indexed_child(
    parent: epm::IndexedRegionImage,
    field: admission::IndexedCanonicalFieldModel,
) -> Option<epm::IndexedRegionImage> {
    if !epm::indexed_image_wf(parent)
        || field.payload_start > field.payload_end
        || field.payload_end as int > parent.bytes.len()
    {
        None
    } else {
        let child = epm::indexed_subview(parent, epm::ByteSpan {
            start: field.payload_start, end: field.payload_end,
        });
        if child.bytes == field.payload_bytes { Some(child) } else { None }
    }
}

pub open spec fn product_admission_source_anchor(
    anchor: epm::SourceAnchor,
) -> admission::AdmissionSourceAnchor {
    admission::AdmissionSourceAnchor {
        source: anchor.source,
        offset: anchor.offset,
    }
}

pub open spec fn product_admission_source_image(
    image: epm::IndexedRegionImage,
) -> admission::AdmissionIndexedSourceImage {
    admission::AdmissionIndexedSourceImage {
        bytes: image.bytes,
        positions: Seq::new(image.positions.len(), |i: int|
            product_admission_source_anchor(image.positions[i])),
        end: product_admission_source_anchor(image.end),
    }
}

pub open spec fn product_wire_indexed_source_agrees(
    cursor: wire_source::WireRuntimeCursorImage,
    image: epm::IndexedRegionImage,
) -> bool {
    match wire_source::wire_runtime_project_region(cursor.message) {
        None => false,
        Some(region) => {
            &&& region.bytes == image.bytes
            &&& region.anchors.len() == image.positions.len()
            &&& region.end.source as usize == image.end.source
            &&& region.end.offset == image.end.offset
            &&& forall|i: int| 0 <= i < region.anchors.len() ==> {
                &&& region.anchors[i].source as usize == image.positions[i].source
                &&& region.anchors[i].offset == image.positions[i].offset
            }
        },
    }
}

pub proof fn product_wire_indexed_field_anchor_exact(
    cursor: wire_source::WireRuntimeCursorImage,
    image: epm::IndexedRegionImage,
    concrete_after: wire_source::WireConcreteCursorImage,
    field: admission::IndexedCanonicalFieldModel,
    start: wire_source::WirePhysicalPosition,
)
    requires
        epm::indexed_image_wf(image),
        product_wire_indexed_source_agrees(cursor, image),
        wire_source::wire_runtime_next_field(cursor)
            == Some(wire_source::WireConcreteTransition::Field {
                state: concrete_after, field, start,
            }),
    ensures {
        let position = admission::admission_indexed_source_position(
            product_admission_source_image(image), field.encoded_start,
        ).unwrap();
        &&& start.logical_offset == field.encoded_start
        &&& position.source == start.physical.source as usize
        &&& position.offset == start.physical.offset
    },
{
    let region = wire_source::wire_runtime_project_region(cursor.message).unwrap();
    let derived = wire_source::wire_runtime_derive_cursor(cursor).unwrap();
    assert(wire_source::wire_backing_project(derived.message) == Some(region));
    assert(wire_source::wire_concrete_next_field_or(derived)
        == Some(wire_source::WireConcreteTransition::Field {
            state: concrete_after, field, start,
        }));
    assert(start == wire_source::wire_region_position(
        region, field.encoded_start,
    ).unwrap());
    assert((field.encoded_start as int) < region.bytes.len());
}

pub proof fn product_wire_indexed_source_agrees_same_backing(
    before: wire_source::WireRuntimeCursorImage,
    after: wire_source::WireRuntimeCursorImage,
    image: epm::IndexedRegionImage,
)
    requires
        product_wire_indexed_source_agrees(before, image),
        after.message == before.message,
    ensures product_wire_indexed_source_agrees(after, image),
{}

pub open spec fn product_indexed_start_anchor(
    image: epm::IndexedRegionImage,
) -> epm::SourceAnchor {
    if image.bytes.len() == 0 { image.end } else { image.positions[0] }
}

pub open spec fn product_frame_source_anchors_match(
    state: ProductScoreState,
) -> bool {
    state.indexed_regions.len() == state.bridge.admission.frames.len()
        && forall|i: int| 0 <= i < state.indexed_regions.len() ==> {
            &&& epm::indexed_image_wf(state.indexed_regions[i])
            &&& state.bridge.admission.frames[i].source_bytes
                == state.indexed_regions[i].bytes
            &&& state.bridge.admission.frames[i].physical_start
                == product_indexed_start_anchor(state.indexed_regions[i]).offset
            &&& state.bridge.admission.frames[i].physical_source
                == product_indexed_start_anchor(state.indexed_regions[i]).source
        }
}

pub proof fn product_frame_source_anchors_root(
    state: ProductScoreState,
)
    requires
        state.indexed_regions.len() == 1,
        state.bridge.admission.frames.len() == 1,
        epm::indexed_image_wf(state.indexed_regions[0]),
        state.bridge.admission.frames[0].source_bytes
            == state.indexed_regions[0].bytes,
        state.bridge.admission.frames[0].physical_start
            == product_indexed_start_anchor(state.indexed_regions[0]).offset,
        state.bridge.admission.frames[0].physical_source
            == product_indexed_start_anchor(state.indexed_regions[0]).source,
    ensures product_frame_source_anchors_match(state),
{}

pub proof fn product_frame_source_anchors_push(
    before: ProductScoreState,
    after: ProductScoreState,
)
    requires
        product_frame_source_anchors_match(before),
        after.indexed_regions.len() == before.indexed_regions.len() + 1,
        after.bridge.admission.frames.len()
            == before.bridge.admission.frames.len() + 1,
        after.indexed_regions.drop_last() == before.indexed_regions,
        before.bridge.admission.frames.len() > 0,
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 1
            ==> after.bridge.admission.frames[i]
                == before.bridge.admission.frames[i],
        after.bridge.admission.frames[before.bridge.admission.frames.len() - 1]
            .source_bytes == before.bridge.admission.frames.last().source_bytes,
        after.bridge.admission.frames[before.bridge.admission.frames.len() - 1]
            .physical_start == before.bridge.admission.frames.last().physical_start,
        after.bridge.admission.frames[before.bridge.admission.frames.len() - 1]
            .physical_source == before.bridge.admission.frames.last().physical_source,
        epm::indexed_image_wf(after.indexed_regions.last()),
        after.bridge.admission.frames.last().source_bytes
            == after.indexed_regions.last().bytes,
        after.bridge.admission.frames.last().physical_start
            == product_indexed_start_anchor(after.indexed_regions.last()).offset,
        after.bridge.admission.frames.last().physical_source
            == product_indexed_start_anchor(after.indexed_regions.last()).source,
    ensures product_frame_source_anchors_match(after),
{
    assert forall|i: int| 0 <= i < after.indexed_regions.len()
        implies {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.bridge.admission.frames[i].source_bytes
                == after.indexed_regions[i].bytes
            &&& after.bridge.admission.frames[i].physical_start
                == product_indexed_start_anchor(after.indexed_regions[i]).offset
            &&& after.bridge.admission.frames[i].physical_source
                == product_indexed_start_anchor(after.indexed_regions[i]).source
        }
    by {
        if i < before.indexed_regions.len() - 1 {
            assert(after.indexed_regions[i] == before.indexed_regions[i]);
            assert(after.bridge.admission.frames[i]
                == before.bridge.admission.frames[i]);
        } else if i == before.indexed_regions.len() - 1 {
            assert(after.indexed_regions[i] == before.indexed_regions[i]);
        }
    }
}

pub proof fn product_frame_source_anchors_pop(
    before: ProductScoreState,
    after: ProductScoreState,
)
    requires
        product_frame_source_anchors_match(before),
        before.indexed_regions.len() > 1,
        after.indexed_regions == before.indexed_regions.drop_last(),
        after.bridge.admission.frames.len()
            == before.bridge.admission.frames.len() - 1,
        forall|i: int| 0 <= i < after.bridge.admission.frames.len() - 1
            ==> after.bridge.admission.frames[i]
                == before.bridge.admission.frames[i],
        after.bridge.admission.frames.last().source_bytes
            == before.bridge.admission.frames[
                before.bridge.admission.frames.len() - 2
            ].source_bytes,
        after.bridge.admission.frames.last().physical_start
            == before.bridge.admission.frames[
                before.bridge.admission.frames.len() - 2
            ].physical_start,
        after.bridge.admission.frames.last().physical_source
            == before.bridge.admission.frames[
                before.bridge.admission.frames.len() - 2
            ].physical_source,
    ensures product_frame_source_anchors_match(after),
{
    assert forall|i: int| 0 <= i < after.indexed_regions.len()
        implies {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.bridge.admission.frames[i].source_bytes
                == after.indexed_regions[i].bytes
            &&& after.bridge.admission.frames[i].physical_start
                == product_indexed_start_anchor(after.indexed_regions[i]).offset
            &&& after.bridge.admission.frames[i].physical_source
                == product_indexed_start_anchor(after.indexed_regions[i]).source
        }
    by {
        assert(after.indexed_regions[i] == before.indexed_regions[i]);
        if i < after.indexed_regions.len() - 1 {
            assert(after.bridge.admission.frames[i]
                == before.bridge.admission.frames[i]);
        }
    }
}

pub proof fn product_indexed_child_source_anchor_exact(
    frame: admission::AdmissionFrameState,
    image: epm::IndexedRegionImage,
    field: admission::IndexedCanonicalFieldModel,
)
    requires
        epm::indexed_image_wf(image),
        frame.source_bytes == image.bytes,
        admission::indexed_field_at_cursor(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: frame.region_start,
                region_end: frame.region_end,
                next: frame.cursor,
                source_bytes: frame.source_bytes,
            }, field,
        ),
    ensures
        product_indexed_child(image, field).is_some(),
        admission::admission_indexed_field_child_source(
            frame, product_admission_source_image(image), field,
        ) == Some(product_admission_source_image(
            product_indexed_child(image, field).unwrap(),
        )),
        admission::admission_indexed_source_position(
            product_admission_source_image(
                product_indexed_child(image, field).unwrap(),
            ), 0,
        ) == Some(product_admission_source_anchor(
            epm::indexed_position_at(image, field.payload_start),
        )),
{
    let source = product_admission_source_image(image);
    assert(admission::admission_indexed_source_wf(source));
    admission::admission_indexed_field_child_source_exact(frame, source, field);
    let child = product_indexed_child(image, field).unwrap();
    let projected = product_admission_source_image(child);
    assert(projected.bytes == source.bytes.subrange(
        field.payload_start as int, field.payload_end as int,
    ));
    assert forall|i: int| 0 <= i < projected.positions.len()
        implies projected.positions[i] == source.positions[field.payload_start as int + i]
    by {}
    assert(projected.positions == source.positions.subrange(
        field.payload_start as int, field.payload_end as int,
    ));
    assert(projected.end == admission::admission_indexed_source_position(
        source, field.payload_end,
    ).unwrap());
    assert(admission::admission_indexed_field_child_source(frame, source, field)
        == Some(projected));
}

#[cfg(rpb3_live_push)]
pub proof fn product_wire_indexed_child_source_ids_exact(
    parent: wire_source::WireRuntimeCursorImage,
    image: epm::IndexedRegionImage,
    field: admission::IndexedCanonicalFieldModel,
)
    requires
        epm::indexed_image_wf(image),
        product_wire_indexed_source_agrees(parent, image),
        product_indexed_child(image, field).is_some(),
        live_runtime_child_cursor(parent, field).is_some(),
    ensures product_wire_indexed_source_agrees(
        live_runtime_child_cursor(parent, field).unwrap(),
        product_indexed_child(image, field).unwrap(),
    ),
{
    let region = wire_source::wire_runtime_project_region(parent.message).unwrap();
    let child_image = product_indexed_child(image, field).unwrap();
    wire_source::wire_runtime_subview_exact(
        parent.message, field.payload_start, field.payload_end,
    );
    let child_cursor = live_runtime_child_cursor(parent, field).unwrap();
    let child_region = wire_source::wire_runtime_project_region(child_cursor.message).unwrap();
    assert(child_region.bytes == image.bytes.subrange(
        field.payload_start as int, field.payload_end as int,
    ));
    assert(child_region.anchors.len() == child_image.positions.len());
    assert forall|i: int| 0 <= i < child_region.anchors.len() implies {
        &&& child_region.anchors[i].source as usize == child_image.positions[i].source
        &&& child_region.anchors[i].offset == child_image.positions[i].offset
    } by {
        assert(child_region.anchors[i]
            == region.anchors[field.payload_start as int + i]);
        assert(child_image.positions[i]
            == image.positions[field.payload_start as int + i]);
    }
    if field.payload_end as int == image.bytes.len() {
        assert(wire_source::wire_region_position(
            region, field.payload_end,
        ).unwrap().physical == region.end);
        assert(child_image.end == image.end);
    } else {
        assert(wire_source::wire_region_position(
            region, field.payload_end,
        ).unwrap().physical == region.anchors[field.payload_end as int]);
        assert(child_image.end == image.positions[field.payload_end as int]);
    }
    assert(child_region.end.source as usize == child_image.end.source);
    assert(child_region.end.offset == child_image.end.offset);
}

pub open spec fn product_map_regions(
    before: ProductScoreState,
    action: map::IntegratedAdmissionAction,
    outcome: map::IntegratedAdmissionOutcome,
) -> Option<Seq<epm::IndexedRegionImage>> {
    match outcome {
        map::IntegratedAdmissionOutcome::MessagePushed { .. } => match action {
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Push { field, .. },
            } => match product_indexed_child(before.indexed_regions.last(), field) {
                Some(child) => Some(before.indexed_regions.push(child)),
                None => None,
            },
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::PushIndexed { field, .. },
            } => match product_indexed_child(before.indexed_regions.last(), field) {
                Some(child) => Some(before.indexed_regions.push(child)),
                None => None,
            },
            _ => None,
        },
        map::IntegratedAdmissionOutcome::MapValuePushed { .. } => match action {
            map::IntegratedAdmissionAction::MapValue { field, .. } =>
                match product_indexed_child(before.indexed_regions.last(), field) {
                    Some(child) => Some(before.indexed_regions.push(child)),
                    None => None,
                },
            map::IntegratedAdmissionAction::MapValueIndexed { field, .. } =>
                match product_indexed_child(before.indexed_regions.last(), field) {
                    Some(child) => Some(before.indexed_regions.push(child)),
                    None => None,
                },
            _ => None,
        },
        map::IntegratedAdmissionOutcome::MessageParentAccepted { .. }
        | map::IntegratedAdmissionOutcome::MapValueAccepted { .. }
        | map::IntegratedAdmissionOutcome::MapEntryCompleted { .. } =>
            Some(before.indexed_regions.drop_last()),
        map::IntegratedAdmissionOutcome::RootFinished { .. } => None,
        _ => Some(before.indexed_regions),
    }
}

pub open spec fn product_map_epm_frames(
    before: ProductScoreState,
    outcome: map::IntegratedAdmissionOutcome,
) -> Option<Seq<Option<epm::EpmFrame>>> {
    match outcome {
        map::IntegratedAdmissionOutcome::MessagePushed { .. }
        | map::IntegratedAdmissionOutcome::MapValuePushed { .. } =>
            Some(before.epm_frames.push(None)),
        map::IntegratedAdmissionOutcome::MessageParentAccepted { .. }
        | map::IntegratedAdmissionOutcome::MapValueAccepted { .. }
        | map::IntegratedAdmissionOutcome::MapEntryCompleted { .. } =>
            Some(before.epm_frames.drop_last()),
        map::IntegratedAdmissionOutcome::RootFinished { .. } => None,
        _ => Some(before.epm_frames),
    }
}

pub open spec fn product_map_pop_owned(before: ProductScoreState) -> bool {
    before.epm_frames.len() > 0
        && before.epm_frames.last().is_none()
        && (before.epm_frames.len() <= 1
            || before.epm_frames[before.epm_frames.len() - 2].is_none())
}

pub open spec fn product_map_step(
    before: ProductScoreState,
    after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
) -> bool {
    match map::integrated_admission_driver_step(before.map, action) {
        Some(outcome) => match (
            map::integrated_nonterminal_state(outcome),
            product_map_regions(before, action, outcome),
            product_map_epm_frames(before, outcome),
        ) {
            (Some(next), Some(regions), Some(epm_frames)) => {
                &&& match action {
                    map::IntegratedAdmissionAction::Message {
                        action: admission::ConcreteFrameActionModel::Push { .. },
                    }
                    | map::IntegratedAdmissionAction::MapValue { .. } => {
                        &&& before.indexed_regions.len() > 0
                        &&& next.admission.frames.len() > 0
                        &&& next.admission.frames.last().physical_source
                            == product_indexed_start_anchor(regions.last()).source
                        &&& next.admission.frames.last().physical_start
                            == product_indexed_start_anchor(regions.last()).offset
                    },
                    map::IntegratedAdmissionAction::Message {
                        action: admission::ConcreteFrameActionModel::PushIndexed {
                            parent_source, ..
                        },
                    } => before.indexed_regions.len() > 0
                        && parent_source == product_admission_source_image(
                            before.indexed_regions.last(),
                        ),
                    map::IntegratedAdmissionAction::Message {
                        action: admission::ConcreteFrameActionModel::ContinueIndexed {
                            parent_source, ..
                        },
                    } => before.indexed_regions.len() > 0
                        && parent_source == product_admission_source_image(
                            before.indexed_regions.last(),
                        ),
                    map::IntegratedAdmissionAction::MapValueIndexed {
                        parent_source, ..
                    } => before.indexed_regions.len() > 0
                        && parent_source == product_admission_source_image(
                            before.indexed_regions.last(),
                        ),
                    _ => true,
                }
                &&& match outcome {
                    map::IntegratedAdmissionOutcome::MessageParentAccepted { .. }
                    | map::IntegratedAdmissionOutcome::MapValueAccepted { .. }
                    | map::IntegratedAdmissionOutcome::MapEntryCompleted { .. } =>
                        product_map_pop_owned(before),
                    _ => true,
                }
                &&& after.map == next
                &&& after.bridge.admission == next.admission
                &&& after.bridge.transcript == next.transcript
                &&& after.indexed_regions == regions
                &&& after.epm_frames == epm_frames
                &&& after.accepted_epm_sources
                    == product_epm_receipts_carried(before, after)
            },
            _ => false,
        },
        None => false,
    }
}

pub proof fn product_map_indexed_push_anchor_exact(
    before: ProductScoreState,
    after: ProductScoreState,
    field: admission::IndexedCanonicalFieldModel,
    offered: admission::AdmissionFrameState,
    parent_source: admission::AdmissionIndexedSourceImage,
    frame_bytes: usize,
)
    requires product_map_step(
        before, after,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::PushIndexed {
                field, child: offered, parent_source, frame_bytes,
            },
        },
    ),
    ensures
        parent_source == product_admission_source_image(before.indexed_regions.last()),
        product_indexed_child(before.indexed_regions.last(), field).is_some(),
        after.indexed_regions.last()
            == product_indexed_child(before.indexed_regions.last(), field).unwrap(),
        after.bridge.admission.frames.last().source_bytes
            == after.indexed_regions.last().bytes,
        after.bridge.admission.frames.last().physical_start
            == product_indexed_start_anchor(after.indexed_regions.last()).offset,
        after.bridge.admission.frames.last().physical_source
            == product_indexed_start_anchor(after.indexed_regions.last()).source,
{
    let child_image = product_indexed_child(before.indexed_regions.last(), field).unwrap();
    assert(after.indexed_regions.last() == child_image);
    assert(parent_source == product_admission_source_image(
        before.indexed_regions.last(),
    ));
}

pub proof fn product_map_indexed_continue_anchor_exact(
    before: ProductScoreState, after: ProductScoreState,
    field: admission::IndexedCanonicalFieldModel,
    parent_source: admission::AdmissionIndexedSourceImage,
)
    requires product_map_step(
        before, after,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::ContinueIndexed {
                field, parent_source,
            },
        },
    ),
    ensures {
        let occurrence = after.bridge.admission.frames.last().scalar_occurrences.last();
        let position = admission::admission_indexed_source_position(
            parent_source, field.encoded_start,
        ).unwrap();
        &&& parent_source == product_admission_source_image(
            before.indexed_regions.last(),
        )
        &&& after.indexed_regions == before.indexed_regions
        &&& occurrence.physical_source == position.source
        &&& occurrence.physical_start == position.offset as int
        &&& occurrence.tag == field.number
        &&& occurrence.bytes == field.payload_bytes
        &&& after.bridge.admission.frames.last().cursor == field.encoded_end
    },
{
    assert(map::integrated_admission_driver_step(
        before.map,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::ContinueIndexed {
                field, parent_source,
            },
        },
    ).is_some());
    assert(admission::concrete_message_driver_step(
        before.map.admission,
        admission::ConcreteFrameActionModel::ContinueIndexed {
            field, parent_source,
        },
    ).is_some());
    admission::concrete_message_driver_indexed_continue_anchor_exact(
        before.map.admission, field, parent_source,
    );
}

pub proof fn product_map_value_indexed_push_anchor_exact(
    before: ProductScoreState,
    after: ProductScoreState,
    field: admission::IndexedCanonicalFieldModel,
    offered: admission::AdmissionFrameState,
    parent_source: admission::AdmissionIndexedSourceImage,
    frame_bytes: usize,
)
    requires product_map_step(
        before, after,
        map::IntegratedAdmissionAction::MapValueIndexed {
            field, child: offered, parent_source, frame_bytes,
        },
    ),
    ensures
        parent_source == product_admission_source_image(before.indexed_regions.last()),
        product_indexed_child(before.indexed_regions.last(), field).is_some(),
        after.indexed_regions.last()
            == product_indexed_child(before.indexed_regions.last(), field).unwrap(),
        after.bridge.admission.frames.last().source_bytes
            == after.indexed_regions.last().bytes,
        after.bridge.admission.frames.last().physical_start
            == product_indexed_start_anchor(after.indexed_regions.last()).offset,
        after.bridge.admission.frames.last().physical_source
            == product_indexed_start_anchor(after.indexed_regions.last()).source,
{
    let child_image = product_indexed_child(before.indexed_regions.last(), field).unwrap();
    assert(after.indexed_regions.last() == child_image);
    assert(parent_source == product_admission_source_image(
        before.indexed_regions.last(),
    ));
}

pub open spec fn product_linked_top(state: ProductScoreState)
    -> Option<epm::LinkedEpmState> {
    if state.epm_frames.len() == 0 { None } else {
        match state.epm_frames.last() {
            None => None,
            Some(frame) => Some(epm::LinkedEpmState {
                epm: frame,
                admission: state.bridge.admission,
                indexed_regions: state.indexed_regions,
            }),
        }
    }
}

pub open spec fn product_linked_parent(state: ProductScoreState)
    -> Option<epm::LinkedEpmState> {
    if state.epm_frames.len() <= 1 { None } else {
        match state.epm_frames[state.epm_frames.len() - 2] {
            None => None,
            Some(frame) => Some(epm::LinkedEpmState {
                epm: frame,
                admission: state.bridge.admission,
                indexed_regions: state.indexed_regions,
            }),
        }
    }
}

pub enum ProductSourceAction {
    Map { action: map::IntegratedAdmissionAction },
    RootScore,
    RootEPathMapScore,
    ChildEPathMapScore,
    EpmHandoff {
        backing: epm::IndexedBackingImage,
        plan: Option<epm::ActPlanImage>,
        validated: Option<epm::ValidatedActPlanImage>,
    },
    EpmContinue { event: epm::EpmEvent },
    EpmPush {
        event: epm::EpmEvent,
        action: epm::EpmAction,
        child: admission::AdmissionFrameState,
    },
    EpmScore,
    EpmChildReturn {
        summary: epm::ChildSummary,
        admission_summary: admission::AdmissionReturnedKind,
        nested_after: Option<epm::EpmFrame>,
    },
    EpmComplete { action: epm::EpmAction },
    EpmFinishAccept,
}

pub open spec fn product_epm_handoff_step(
    before: ProductScoreState, after: ProductScoreState,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
) -> bool {
    before.epm_frames.len() > 0
        && before.epm_frames.last().is_none()
        && match epm::linked_map_epm_handoff(
            before.bridge.admission, before.indexed_regions, backing, plan, validated,
        ) {
            None => false,
            Some(linked) => {
                &&& linked.admission == before.bridge.admission
                &&& linked.indexed_regions == before.indexed_regions
                &&& score_bridge::bridge_same(before.bridge, after.bridge)
                &&& after.map == before.map
                &&& after.indexed_regions == before.indexed_regions
                &&& after.epm_frames == before.epm_frames.update(
                    before.epm_frames.len() - 1, Some(linked.epm),
                )
                &&& after.accepted_epm_sources
                    == product_epm_receipts_carried(before, after)
            },
        }
}

pub open spec fn product_epm_continue_step(
    before: ProductScoreState, after: ProductScoreState,
    event: epm::EpmEvent,
) -> bool {
    match (product_linked_top(before), product_linked_top(after)) {
        (Some(prior), Some(next)) => {
            &&& epm::linked_epm_continue(prior, next, event)
            &&& after.map.admission == next.admission
            &&& after.map.transcript == before.map.transcript
            &&& after.map.map_frames == before.map.map_frames
            &&& after.bridge.admission == next.admission
            &&& after.bridge.transcript == before.bridge.transcript
            &&& after.indexed_regions == before.indexed_regions
            &&& after.epm_frames == before.epm_frames.update(
                before.epm_frames.len() - 1, Some(next.epm),
            )
            &&& after.accepted_epm_sources
                == product_epm_receipts_carried(before, after)
        },
        _ => false,
    }
}

pub open spec fn product_epm_push_step(
    before: ProductScoreState, after: ProductScoreState,
    event: epm::EpmEvent, action: epm::EpmAction,
    child: admission::AdmissionFrameState,
) -> bool {
    match (product_linked_top(before), product_linked_parent(after)) {
        (Some(prior), Some(next)) => {
            &&& epm::linked_epm_push(prior, next, event, action, child)
            &&& after.map.admission == next.admission
            &&& after.map.transcript == before.map.transcript
            &&& after.map.map_frames == before.map.map_frames.push(None)
            &&& after.bridge.admission == next.admission
            &&& after.bridge.transcript == before.bridge.transcript
            &&& after.epm_frames == before.epm_frames.update(
                before.epm_frames.len() - 1, Some(next.epm),
            ).push(None)
            &&& after.accepted_epm_sources
                == product_epm_receipts_carried(before, after)
        },
        _ => false,
    }
}

pub open spec fn product_epm_score_step(
    before: ProductScoreState, after: ProductScoreState,
) -> bool {
    &&& product_epm_score_owned(before)
    &&& score_bridge::bridge_score_source_step(before.bridge, after.bridge)
    &&& after.map.admission == after.bridge.admission
    &&& after.map.transcript == after.bridge.transcript
    &&& after.map.map_frames == before.map.map_frames
    &&& after.epm_frames == before.epm_frames
    &&& after.indexed_regions == before.indexed_regions
    &&& after.accepted_epm_sources == product_epm_receipts_carried(before, after)
}

pub open spec fn product_root_score_step(
    before: ProductScoreState, after: ProductScoreState,
) -> bool {
    &&& product_score_wf(before)
    &&& before.bridge.admission.frames.len() == 1
    &&& before.bridge.admission.frames[0].frame_kind == 1
    &&& before.epm_frames[0].is_none()
    &&& score_bridge::bridge_score_source_step(before.bridge, after.bridge)
    &&& after.map.admission == after.bridge.admission
    &&& after.map.transcript == after.bridge.transcript
    &&& after.map.map_frames == before.map.map_frames
    &&& after.epm_frames == before.epm_frames
    &&& after.indexed_regions == before.indexed_regions
    &&& after.accepted_epm_sources == product_epm_receipts_carried(before, after)
}

// A message child is scored while it is still on the stack.  Its parent
// acceptance is a subsequent transition, so the indexed backing and frame
// topology stay unchanged during this source-selected score append.
pub open spec fn product_message_child_score_step(
    before: ProductScoreState, after: ProductScoreState,
) -> bool {
    &&& product_score_wf(before)
    &&& before.bridge.admission.frames.len() > 1
    &&& before.bridge.admission.frames.last().frame_kind == 1
    &&& before.map.map_frames.last().is_none()
    &&& score_bridge::bridge_score_source_step(before.bridge, after.bridge)
    &&& after.map.admission == after.bridge.admission
    &&& after.map.transcript == after.bridge.transcript
    &&& after.map.map_frames == before.map.map_frames
    &&& after.epm_frames == before.epm_frames
    &&& after.indexed_regions == before.indexed_regions
    &&& after.accepted_epm_sources == product_epm_receipts_carried(before, after)
}

pub proof fn product_root_score_shape_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
)
    requires
        product_score_wf(before),
        before.bridge.admission.frames.len() == 1,
        before.bridge.admission.frames[0].frame_kind == 1,
        before.epm_frames[0].is_none(),
        score_bridge::bridge_well_formed(after.bridge),
        admission::admission_state_wf(after.bridge.admission),
        score_bridge::bridge_score_step_shape(before.bridge, after.bridge),
        after.map.admission == after.bridge.admission,
        after.map.transcript == after.bridge.transcript,
        after.map.map_frames == before.map.map_frames,
        after.epm_frames == before.epm_frames,
        after.indexed_regions == before.indexed_regions,
        after.accepted_epm_sources == product_epm_receipts_carried(before, after),
    ensures
        product_score_wf(after),
        after.bridge.admission.frames.len() == 1,
        after.bridge.admission.frames[0].frame_kind == 1,
        after.indexed_regions[0].bytes
            == after.bridge.admission.frames[0].source_bytes,
{
    product_epm_receipts_carried_preserves(before, after);
    assert(before.map.map_frames.len() == 1);
    assert(before.map.map_frames[0].is_none());
    assert(after.map.map_frames[0].is_none());
    assert(score_bridge::bridge_well_formed(after.bridge));
    assert(map::integrated_admission_wf(after.map));
    assert(after.epm_frames.len() == after.bridge.admission.frames.len());
    assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
    assert(after.epm_frames[0].is_none());
    assert(after.indexed_regions[0] == before.indexed_regions[0]);
    assert(after.bridge.admission.frames[0].source_bytes
        == before.bridge.admission.frames[0].source_bytes);
    assert(after.bridge.admission.frames[0].physical_source
        == before.bridge.admission.frames[0].physical_source);
    assert(after.bridge.admission.frames[0].physical_start
        == before.bridge.admission.frames[0].physical_start);
    assert(product_frame_source_anchors_match(after));
}

pub proof fn product_root_score_step_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
)
    requires product_root_score_step(before, after),
    ensures
        product_score_wf(after),
        after.bridge.admission.frames.len() == 1,
        after.bridge.admission.frames[0].frame_kind == 1,
        after.indexed_regions[0].bytes
            == after.bridge.admission.frames[0].source_bytes,
{
    score_bridge::bridge_score_source_step_preserves_exact(
        before.bridge, after.bridge,
    );
    score_bridge::bridge_score_source_step_preserves_shape(
        before.bridge, after.bridge,
    );
    product_root_score_shape_preserves_exact(before, after);
}

pub open spec fn product_epm_score_owned(state: ProductScoreState) -> bool {
    state.bridge.admission.frames.len() > 0
        && state.epm_frames.len() == state.bridge.admission.frames.len()
        && state.map.map_frames.len() == state.bridge.admission.frames.len()
        && state.bridge.admission.frames.last().frame_kind == 1
        && state.map.map_frames.last().is_none()
        && exists|owner: int| 0 <= owner < state.epm_frames.len() - 1
            && state.epm_frames[owner].is_some()
            && (forall|i: int| owner < i < state.epm_frames.len()
                ==> state.map.map_frames[i].is_none())
}

#[verifier::opaque]
pub open spec fn product_epm_source_child_binding(
    before: ProductScoreState,
    prior: epm::LinkedEpmState,
    next: epm::LinkedEpmState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
) -> bool {
    match epm_nested_source::source_derived_epm_return(prior, before.epm_frames) {
        None => false,
        Some(derived) => {
            &&& summary == derived.summary
            &&& admission_summary == derived.admission_summary
            &&& before.epm_frames.last() == derived.nested_before
            &&& nested_after == derived.nested_after
            &&& epm_nested_source::source_linked_epm_child_return(
                prior, next, before.epm_frames,
            )
        },
    }
}

pub open spec fn product_epm_child_return_step(
    before: ProductScoreState, after: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
) -> bool {
    match (product_linked_parent(before), product_linked_top(after)) {
        (Some(prior), Some(next)) => {
            &&& product_epm_source_child_binding(
                before, prior, next, summary, admission_summary, nested_after,
            )
            &&& epm::linked_epm_child_return(
                prior, next, summary, admission_summary,
                before.epm_frames.last(), nested_after,
            )
            &&& after.map.admission == next.admission
            &&& after.map.transcript == before.map.transcript
            &&& after.map.map_frames == before.map.map_frames.drop_last()
            &&& after.bridge.admission == next.admission
            &&& after.bridge.transcript == before.bridge.transcript
            &&& after.epm_frames == before.epm_frames.drop_last().update(
                before.epm_frames.len() - 2, Some(next.epm),
            )
            &&& after.accepted_epm_sources
                == product_epm_receipts_carried(before, after)
        },
        _ => false,
    }
}

pub proof fn product_epm_child_return_source_exact(
    before: ProductScoreState, after: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
)
    requires product_epm_child_return_step(
        before, after, summary, admission_summary, nested_after,
    ),
    ensures match (product_linked_parent(before), product_linked_top(after)) {
        (Some(prior), Some(next)) => {
            &&& epm_nested_source::source_derived_epm_return(
                prior, before.epm_frames,
            ).is_some()
            &&& epm_nested_source::source_linked_epm_child_return(
                prior, next, before.epm_frames,
            )
        },
        _ => false,
    },
{
    if let (Some(prior), Some(next)) = (
        product_linked_parent(before), product_linked_top(after),
    ) {
        reveal(product_epm_source_child_binding);
    }
}

pub open spec fn product_epm_complete_step(
    before: ProductScoreState, after: ProductScoreState,
    action: epm::EpmAction,
) -> bool {
    match (product_linked_top(before), product_linked_top(after)) {
        (Some(prior), Some(next)) => {
            &&& epm::linked_epm_complete(prior, next, action)
            &&& after.map == before.map
            &&& score_bridge::bridge_same(before.bridge, after.bridge)
            &&& after.indexed_regions == before.indexed_regions
            &&& after.epm_frames == before.epm_frames.update(
                before.epm_frames.len() - 1, Some(next.epm),
            )
            &&& after.accepted_epm_sources
                == product_epm_receipts_carried(before, after)
        },
        _ => false,
    }
}

pub open spec fn product_epm_finish_accept_step(
    before: ProductScoreState, after: ProductScoreState,
) -> bool {
    match product_linked_top(before) {
        None => false,
        Some(prior) => match epm_source_accept::epm_source_finish_accept(prior) {
            None => false,
            Some(next_admission) => {
                &&& after.bridge.admission == next_admission
                &&& after.bridge.transcript == before.bridge.transcript
                &&& after.map.admission == next_admission
                &&& after.map.transcript == before.map.transcript
                &&& after.map.map_frames == before.map.map_frames.drop_last()
                &&& after.epm_frames == before.epm_frames.drop_last()
                &&& after.indexed_regions == before.indexed_regions.drop_last()
                &&& after.accepted_epm_sources
                    == product_epm_receipts_carried(before, after).update(
                        after.bridge.admission.frames.len() - 1, Some(prior),
                    )
            },
        },
    }
}

pub proof fn product_epm_finish_receipt_preserves(
    before: ProductScoreState, after: ProductScoreState,
)
    requires
        product_epm_source_receipts_wf(before),
        product_epm_finish_accept_step(before, after),
    ensures product_epm_source_receipts_wf(after),
{
    let prior = product_linked_top(before).unwrap();
    let accepted = epm_source_accept::epm_source_finish_accept(prior).unwrap();
    let finished = epm_source_accept::epm_end_linked(prior);
    assert(epm_source_accept::epm_source_accepts_into_program31(
        prior, finished, accepted,
    ));
    epm_source_accept::epm_source_accept_carries_end_summary(
        prior, finished, accepted,
    );
    let carried = product_epm_receipts_carried(before, after);
    let carried_state = ProductScoreState {
        accepted_epm_sources: carried,
        ..after
    };
    assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
    assert(carried_state.accepted_epm_sources
        == product_epm_receipts_carried(before, carried_state));
    product_epm_receipts_carried_preserves(before, carried_state);
    let parent = after.bridge.admission.frames.len() - 1;
    assert(parent == accepted.frames.len() - 1);
    assert(prior.indexed_regions == before.indexed_regions);
    assert(after.indexed_regions == before.indexed_regions.drop_last());
    assert(prior.indexed_regions.len() > 1);
    assert(after.indexed_regions[parent]
        == prior.indexed_regions[prior.indexed_regions.len() - 2]);
    assert(after.bridge.admission == accepted);
    assert(after.bridge.admission.frames[parent].returns.last().body_bytes
        == prior.epm.snapshot.bytes);
    assert(product_epm_source_receipt_matches(
        after.bridge.admission.frames[parent], after.indexed_regions[parent], prior,
    ));
    assert forall|i: int| 0 <= i < after.accepted_epm_sources.len()
        implies match #[trigger] after.accepted_epm_sources[i] {
            None => true,
            Some(source) => product_epm_source_receipt_matches(
                after.bridge.admission.frames[i], after.indexed_regions[i], source,
            ),
        } by {
        if i != parent {
            assert(after.accepted_epm_sources[i] == carried_state.accepted_epm_sources[i]);
        }
    }
}

pub open spec fn product_epm_source_witness_ready(
    source: epm::LinkedEpmState,
) -> bool {
    &&& admission::admission_state_wf(source.admission)
    &&& source.admission.frames.len() == 2
    &&& source.admission.arena.len() == 0
    &&& source.admission.frames[0].frame_kind == 1
    &&& source.admission.frames[1].frame_kind == 3
    &&& epm::linked_indexed_sources(source)
    &&& source.epm.snapshot == source.indexed_regions[1]
    &&& epm::frame_layout_wf(source.epm)
    &&& epm_source_accept::epm_source_finish_accept(source).is_some()
}

pub open spec fn product_epm_source_witness_before(
    source: epm::LinkedEpmState,
) -> ProductScoreState {
    let bridge = score_bridge::ScoreAdmissionBridge {
        admission: source.admission,
        transcript: rpb3_score_transcript::empty_arena(),
    };
    ProductScoreState {
        bridge,
        map: map::IntegratedAdmissionState {
            admission: source.admission,
            map_frames: seq![None, None],
            transcript: bridge.transcript,
        },
        epm_frames: seq![None, Some(source.epm)],
        indexed_regions: source.indexed_regions,
        accepted_epm_sources: seq![None, None],
    }
}

pub open spec fn product_epm_source_witness_after(
    source: epm::LinkedEpmState,
) -> ProductScoreState
    recommends epm_source_accept::epm_source_finish_accept(source).is_some(),
{
    let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
    let before = product_epm_source_witness_before(source);
    let bridge = score_bridge::ScoreAdmissionBridge {
        admission: accepted,
        transcript: before.bridge.transcript,
    };
    ProductScoreState {
        bridge,
        map: map::IntegratedAdmissionState {
            admission: accepted,
            map_frames: seq![None],
            transcript: bridge.transcript,
        },
        epm_frames: seq![None],
        indexed_regions: source.indexed_regions.drop_last(),
        accepted_epm_sources: seq![Some(source)],
    }
}

pub proof fn product_epm_source_witness_lifts_accept(
    source: epm::LinkedEpmState,
)
    requires product_epm_source_witness_ready(source),
    ensures
        product_score_wf(product_epm_source_witness_before(source)),
        product_epm_finish_accept_step(
            product_epm_source_witness_before(source),
            product_epm_source_witness_after(source),
        ),
        product_score_wf(product_epm_source_witness_after(source)),
        product_epm_source_witness_after(source).accepted_epm_sources[0]
            == Some(source),
{
    rpb3_score_transcript::empty_arena_well_formed();
    let before = product_epm_source_witness_before(source);
    let after = product_epm_source_witness_after(source);
    assert(map::integrated_admission_wf(before.map));
    assert(score_bridge::bridge_well_formed(before.bridge));
    assert(product_epm_source_receipts_wf(before));
    assert(product_score_wf(before));
    assert(product_linked_top(before) == Some(source));
    let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
    let finished = epm_source_accept::epm_end_linked(source);
    assert(epm_source_accept::epm_source_accepts_into_program31(
        source, finished, accepted,
    ));
    epm_source_accept::epm_source_accept_carries_end_summary(
        source, finished, accepted,
    );
    assert(accepted.frames.len() == 1);
    assert(after.bridge.admission == accepted);
    assert(after.map.admission == accepted);
    assert(before.map.map_frames.drop_last() == seq![None]);
    assert(before.epm_frames.drop_last() == seq![None]);
    assert(product_epm_receipts_carried(before, after).len() == 1);
    assert(product_epm_receipts_carried(before, after)[0] == None);
    assert(product_epm_receipts_carried(before, after) == seq![None]);
    assert(after.accepted_epm_sources
        == product_epm_receipts_carried(before, after).update(0, Some(source)));
    assert(product_epm_finish_accept_step(before, after));
    product_epm_finish_accept_preserves_exact(before, after);
}

pub open spec fn product_epm_modeled_empty_snapshot_bytes() -> Seq<u8> {
    seq![0x45u8, 0x50u8, 0x4du8, 0x31u8, 1u8, 0u8, 0u8, 0u8]
}

pub open spec fn product_epm_modeled_empty_backing() -> epm::IndexedBackingImage {
    epm::IndexedBackingImage::Borrowed {
        bytes: product_epm_modeled_empty_snapshot_bytes(),
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 8,
        end_override: None,
    }
}

pub open spec fn product_epm_modeled_empty_source() -> epm::LinkedEpmState {
    let bytes = product_epm_modeled_empty_snapshot_bytes();
    let backing = product_epm_modeled_empty_backing();
    let snapshot = epm::indexed_backing_project(backing).unwrap();
    let required = Set::empty().insert(3);
    let parent = admission::AdmissionFrameState {
        physical_source: 0,
        physical_start: 0,
        program_type_id: 31,
        region_start: 0,
        region_end: 8,
        source_bytes: bytes,
        cursor: 8,
        slot_count: 4,
        last_slot: Some(3),
        seen: required,
        required,
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        family: admission::AdmissionFamily::Other,
        waiting_id: Some(0),
        waiting_kind: 3,
        waiting_slot: Some(3),
        waiting_tag: Some(9),
        return_id: None,
        frame_kind: 1,
        body_id: 0,
        score_handle: None,
        nested_started: 1,
        nested_resolved: 0,
    };
    let child = admission::AdmissionFrameState {
        physical_source: 0,
        physical_start: 0,
        program_type_id: 0,
        region_start: 0,
        region_end: 8,
        source_bytes: bytes,
        cursor: 8,
        slot_count: 0,
        last_slot: None,
        seen: Set::empty(),
        required: Set::empty(),
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        family: admission::AdmissionFamily::Other,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: Some(0),
        frame_kind: 3,
        body_id: 1,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    };
    let admission = admission::AdmissionState {
        frames: seq![parent, child],
        arena: Seq::empty(),
        root_end: 8,
        pushed: 2,
        completed: 0,
        next_obligation_id: 1,
    };
    let prefix = epm::epm_prefix_from_bytes(bytes).unwrap();
    let initial = epm::initial_epm_candidate(backing, snapshot, prefix, None, None);
    let finished_path = epm::PathTopology { root_left: true, ..initial.path };
    let end_cursor = epm::EpmCursorImage { at: 2, ..initial.cursor };
    let epm_frame = epm::EpmFrame {
        path: finished_path,
        cursor: end_cursor,
        ..initial
    };
    epm::LinkedEpmState {
        epm: epm_frame,
        admission,
        indexed_regions: seq![snapshot, snapshot],
    }
}

pub open spec fn product_epm_modeled_empty_nested_source() -> epm::LinkedEpmState {
    let inner = product_epm_modeled_empty_source();
    let bytes = product_epm_modeled_empty_snapshot_bytes();
    let outer = admission::AdmissionFrameState {
        region_end: 8,
        source_bytes: bytes,
        cursor: 8,
        slot_count: 1,
        last_slot: Some(0),
        seen: Set::empty().insert(0),
        waiting_id: Some(0),
        waiting_kind: 1,
        waiting_slot: Some(0),
        waiting_tag: Some(32),
        body_id: 0,
        score_handle: None,
        ..admission::admission_family_witness_frame(
            admission::AdmissionFamily::Expr, Set::empty(), Seq::empty(),
        )
    };
    let parent = admission::AdmissionFrameState {
        return_id: Some(0),
        waiting_id: Some(1),
        body_id: 1,
        ..inner.admission.frames[0]
    };
    let child = admission::AdmissionFrameState {
        return_id: Some(1),
        body_id: 2,
        ..inner.admission.frames[1]
    };
    epm::LinkedEpmState {
        epm: inner.epm,
        admission: admission::AdmissionState {
            frames: seq![outer, parent, child],
            arena: Seq::empty(),
            root_end: 8,
            pushed: 3,
            completed: 0,
            next_obligation_id: 2,
        },
        indexed_regions: seq![inner.epm.snapshot, inner.epm.snapshot, inner.epm.snapshot],
    }
}

pub proof fn product_epm_modeled_empty_nested_source_accepts()
    ensures
        admission::admission_state_wf(
            product_epm_modeled_empty_nested_source().admission,
        ),
        epm::linked_indexed_sources(product_epm_modeled_empty_nested_source()),
        epm_source_accept::epm_source_finish_accept(
            product_epm_modeled_empty_nested_source(),
        ).is_some(),
{
    product_epm_modeled_empty_source_accepts();
    let source = product_epm_modeled_empty_nested_source();
    let outer = source.admission.frames[0];
    let parent = source.admission.frames[1];
    let child = source.admission.frames[2];
    assert(admission::admission_frame_wf(outer));
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    assert(admission::admission_child_physical_contained(outer, parent));
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::admission_state_wf(source.admission));
    assert(epm::linked_indexed_sources(source));
    let finished = epm_source_accept::epm_end_linked(source);
    assert(epm_source_accept::epm_source_end(source, finished));
    assert(epm_source_accept::epm_program31_parent(source.admission));
    assert(admission::admission_complete_child_step(
        source.admission, 1, 3, admission::AdmissionReturnedKind::Other,
    ).is_some());
    assert(epm_source_accept::epm_source_finish_accept(source).is_some());
}

pub open spec fn product_epm_nested_source_witness_before(
    source: epm::LinkedEpmState,
) -> ProductScoreState {
    let bridge = score_bridge::ScoreAdmissionBridge {
        admission: source.admission,
        transcript: rpb3_score_transcript::empty_arena(),
    };
    ProductScoreState {
        bridge,
        map: map::IntegratedAdmissionState {
            admission: source.admission,
            map_frames: seq![None, None, None],
            transcript: bridge.transcript,
        },
        epm_frames: seq![None, None, Some(source.epm)],
        indexed_regions: source.indexed_regions,
        accepted_epm_sources: seq![None, None, None],
    }
}

pub open spec fn product_epm_nested_source_witness_after(
    source: epm::LinkedEpmState,
) -> ProductScoreState
    recommends epm_source_accept::epm_source_finish_accept(source).is_some(),
{
    let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
    let bridge = score_bridge::ScoreAdmissionBridge {
        admission: accepted,
        transcript: rpb3_score_transcript::empty_arena(),
    };
    ProductScoreState {
        bridge,
        map: map::IntegratedAdmissionState {
            admission: accepted,
            map_frames: seq![None, None],
            transcript: bridge.transcript,
        },
        epm_frames: seq![None, None],
        indexed_regions: source.indexed_regions.drop_last(),
        accepted_epm_sources: seq![None, Some(source)],
    }
}

pub proof fn product_epm_modeled_empty_nested_source_lifts_accept()
    ensures {
        let source = product_epm_modeled_empty_nested_source();
        let before = product_epm_nested_source_witness_before(source);
        let after = product_epm_nested_source_witness_after(source);
        &&& product_score_wf(before)
        &&& product_epm_finish_accept_step(before, after)
        &&& product_score_wf(after)
        &&& after.accepted_epm_sources[1] == Some(source)
    },
{
    product_epm_modeled_empty_nested_source_accepts();
    rpb3_score_transcript::empty_arena_well_formed();
    let source = product_epm_modeled_empty_nested_source();
    let before = product_epm_nested_source_witness_before(source);
    let after = product_epm_nested_source_witness_after(source);
    assert(map::integrated_admission_wf(before.map));
    assert(score_bridge::bridge_well_formed(before.bridge));
    assert(product_epm_source_receipts_wf(before));
    assert(product_score_wf(before));
    assert(product_linked_top(before) == Some(source));
    let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
    assert(accepted.frames.len() == 2);
    assert(after.bridge.admission == accepted);
    assert(before.map.map_frames.drop_last() == seq![None, None]);
    assert(before.epm_frames.drop_last() == seq![None, None]);
    assert(product_epm_receipts_carried(before, after) == seq![None, None]);
    assert(after.accepted_epm_sources
        == product_epm_receipts_carried(before, after).update(1, Some(source)));
    assert(product_epm_finish_accept_step(before, after));
    product_epm_finish_accept_preserves_exact(before, after);
}

pub proof fn product_epm_modeled_empty_source_accepts()
    ensures product_epm_source_witness_ready(product_epm_modeled_empty_source()),
{
    let source = product_epm_modeled_empty_source();
    let bytes = product_epm_modeled_empty_snapshot_bytes();
    let backing = product_epm_modeled_empty_backing();
    let snapshot = epm::indexed_backing_project(backing).unwrap();
    let parent = source.admission.frames[0];
    let child = source.admission.frames[1];
    assert(epm::indexed_backing_project(backing) == Some(snapshot));
    assert(snapshot.bytes == bytes);
    assert(epm::indexed_image_wf(snapshot));
    assert(epm::epm_prefix_from_bytes(bytes) == Some(epm::EpmPrefixImage {
        mode: epm::EpmMode::Empty,
        trie_start: 7,
        trie_end: 7,
        value_count: 0,
        value_table_start: 8,
    }));
    assert(admission::admission_epathmap_snapshot_descriptor());
    assert(parent.seen.contains(3));
    assert(parent.required.contains(3));
    assert forall|slot: int| 0 <= slot < 4 implies
        parent.required.contains(slot)
            == (exists|tag: u32|
                admission::admission_required_descriptor_slot(31, tag, slot)) by {
        if slot == 3 {
            assert(admission::admission_required_descriptor_slot(31, 9, 3));
            assert(parent.required.contains(3));
        } else {
            assert(!parent.required.contains(slot));
            assert forall|tag: u32|
                !admission::admission_required_descriptor_slot(31, tag, slot) by {
                assert(!admission::admission_required_descriptor_slot(31, tag, slot));
            }
        }
    }
    assert(admission::admission_required_slots_generated(parent));
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::admission_state_wf(source.admission));
    assert(epm::frame_layout_wf(source.epm));
    assert(epm::linked_indexed_sources(source));
    let finished = epm_source_accept::epm_end_linked(source);
    assert(epm::cursor_event_count(source.epm) == 2);
    assert(epm::path_finish_ok(source.epm.path));
    assert(epm::cursor_event_step(source.epm, finished.epm, epm::EpmEvent::End));
    assert(epm::end_return(
        source.epm,
        epm::EpmAction::Complete {
            mode: source.epm.mode,
            entries_stable: source.epm.entries_stable,
        },
    ));
    assert(epm_source_accept::epm_source_end(source, finished));
    assert(epm_source_accept::epm_program31_parent(source.admission));
    assert(admission::admission_child_summary_agrees(
        child, admission::AdmissionReturnedKind::Other, source.admission.arena,
    ));
    assert(admission::admission_arm_child_summary_agrees(
        parent, child, admission::AdmissionReturnedKind::Other,
        source.admission.arena,
    ));
    assert(admission::admission_complete_child_step(
        source.admission, 0, 3, admission::AdmissionReturnedKind::Other,
    ).is_some());
    assert(epm_source_accept::epm_source_finish_accept(source).is_some());
}

pub proof fn product_epm_modeled_empty_snapshot_score_length()
    ensures
        rpb3_score_transcript::zero_count(product_epm_modeled_empty_snapshot_bytes()) == 3,
        rpb3_score_transcript::bytes_transcript(product_epm_modeled_empty_snapshot_bytes()).len() == 15,
{
    let bytes = product_epm_modeled_empty_snapshot_bytes();
    rpb3_score_transcript::string_and_bytes_transcripts_exact(bytes);
    reveal_with_fuel(rpb3_score_transcript::zero_count, 9);
    assert(rpb3_score_transcript::zero_count(bytes) == 3);
}

pub proof fn product_epm_empty_snapshot_selected_nonempty(
    state: ProductScoreState,
)
    requires
        product_score_wf(state),
        state.bridge.admission.frames.len() > 0,
        state.bridge.admission.frames.last().frame_kind == 1,
        state.epm_frames.last().is_none(),
        state.accepted_epm_sources.last().is_some(),
        state.bridge.admission.frames.last().cursor
            == state.bridge.admission.frames.last().region_end,
        state.accepted_epm_sources.last().unwrap().epm.snapshot.bytes
            == product_epm_modeled_empty_snapshot_bytes(),
        state.bridge.transcript == rpb3_score_transcript::empty_arena(),
        state.bridge.admission.arena.len() == 0,
        score_bridge::bridge_bool_slot(state.bridge.admission.frames.last(), 1, 4)
            == Some(false),
        admission::admission_family_complete(state.bridge.admission.frames.last()),
        admission::admission_slots_complete(state.bridge.admission.frames.last()),
        state.bridge.admission.frames.last().waiting_id.is_none(),
        state.bridge.admission.frames.last().score_handle.is_none(),
    ensures product_epm_source_bridge_finish_selected(state).is_some(),
{
    product_epm_source_receipt_binds_top_eof(state);
    product_epm_modeled_empty_snapshot_score_length();
    let bytes = product_epm_modeled_empty_snapshot_bytes();
    let bytes_kind = rpb3_score_transcript::ScoreNode::BytesAtom(bytes);
    assert(rpb3_score_transcript::arena_well_formed(state.bridge.transcript));
    assert(rpb3_score_transcript::kind_valid(
        state.bridge.transcript.entries, bytes_kind,
    ));
    assert(rpb3_score_transcript::checked_append(
        state.bridge.transcript, bytes_kind,
    ).is_some());
    assert(score_bridge::bridge_append_unattached(state.bridge, bytes_kind).is_some());
    let (after_bytes, bytes_handle) = score_bridge::bridge_append_unattached(
        state.bridge, bytes_kind,
    ).unwrap();
    score_bridge::bridge_append_unattached_exact(state.bridge, bytes_kind);
    score_bridge::bridge_append_unattached_preserves_admission_wf(
        state.bridge, bytes_kind,
    );
    let flag_kind = rpb3_score_transcript::ScoreNode::Integer(0i64);
    rpb3_score_transcript::integer_transcript_exact(0i64);
    assert(score_bridge::bridge_append_unattached(after_bytes, flag_kind).is_some());
    let (after_flag, flag_handle) = score_bridge::bridge_append_unattached(
        after_bytes, flag_kind,
    ).unwrap();
    score_bridge::bridge_append_unattached_exact(after_bytes, flag_kind);
    score_bridge::bridge_append_unattached_preserves_admission_wf(
        after_bytes, flag_kind,
    );
    assert(bytes_handle == 0);
    assert(flag_handle == 1);
    assert(admission::admission_slots_complete(after_flag.admission.frames.last()));
    assert(admission::admission_family_admissible(after_flag.admission.frames.last()));
    let constructor_kind = rpb3_score_transcript::ScoreNode::Integer(13i64);
    assert(score_bridge::bridge_append_unattached(
        after_flag, constructor_kind,
    ).is_some());
    let (after_constructor, constructor_handle) = score_bridge::bridge_append_unattached(
        after_flag, constructor_kind,
    ).unwrap();
    score_bridge::bridge_append_unattached_exact(after_flag, constructor_kind);
    assert(constructor_handle == 2);
    assert(after_constructor.transcript.entries.len() == 3);
    assert(after_constructor.transcript.entries[0].encoded_len == 15);
    assert(after_constructor.transcript.entries[1].encoded_len == 10);
    assert(after_constructor.transcript.entries[2].encoded_len == 10);
    let children = seq![constructor_handle, bytes_handle, flag_handle];
    assert(rpb3_score_transcript::child_handles_valid(
        after_constructor.transcript.entries, children,
    ));
    rpb3_score_transcript::node_transcript_exact(
        after_constructor.transcript.entries, children,
    );
    assert(children.drop_first() == seq![bytes_handle, flag_handle]);
    assert(seq![bytes_handle, flag_handle].drop_first() == seq![flag_handle]);
    assert(seq![flag_handle].drop_first() == Seq::<usize>::empty());
    assert(rpb3_score_transcript::children_encoded_len(
        after_constructor.transcript.entries, Seq::<usize>::empty(),
    ) == 0);
    assert(rpb3_score_transcript::children_encoded_len(
        after_constructor.transcript.entries, seq![flag_handle],
    ) == 10);
    assert(rpb3_score_transcript::children_encoded_len(
        after_constructor.transcript.entries, seq![bytes_handle, flag_handle],
    ) == 25);
    assert(rpb3_score_transcript::children_encoded_len(
        after_constructor.transcript.entries, children,
    ) == 35);
    assert(rpb3_score_transcript::kind_transcript(
        after_constructor.transcript.entries,
        rpb3_score_transcript::ScoreNode::Node(children),
    ).len() == 37);
    assert(score_bridge::bridge_append_unattached(
        after_constructor, rpb3_score_transcript::ScoreNode::Node(children),
    ).is_some());
    let (after_node, score_handle) = score_bridge::bridge_append_unattached(
        after_constructor, rpb3_score_transcript::ScoreNode::Node(children),
    ).unwrap();
    assert(admission::admission_attach_score_step(
        after_node.admission, score_handle,
        state.bridge.admission.frames.last().body_id,
    ).is_some());
    assert(seq![constructor_handle] + seq![bytes_handle, flag_handle] == children);
    assert(score_bridge::bridge_constructor_step(
        after_flag, 13i32, seq![bytes_handle, flag_handle],
    ).is_some());
    assert(state.epm_frames.last().is_none());
    assert(state.accepted_epm_sources.last().unwrap().epm.snapshot.bytes == bytes);
    assert(product_epm_source_bridge_finish_selected(state).is_some());
}

pub proof fn product_epm_modeled_empty_source_scores()
    ensures {
        let source = product_epm_modeled_empty_source();
        let before = product_epm_source_witness_before(source);
        let after = product_epm_source_witness_after(source);
        &&& product_score_wf(before)
        &&& product_epm_finish_accept_step(before, after)
        &&& product_score_wf(after)
        &&& after.accepted_epm_sources[0] == Some(source)
        &&& product_epm_source_root_score_selected(after).is_some()
        &&& product_source_step(
            after,
            product_epm_source_root_score_selected(after).unwrap(),
            ProductSourceAction::RootEPathMapScore,
        )
    },
{
    product_epm_modeled_empty_source_accepts();
    let source = product_epm_modeled_empty_source();
    product_epm_source_witness_lifts_accept(source);
    let after = product_epm_source_witness_after(source);
    product_epm_source_receipt_binds_root_eof(after);
    assert(after.bridge.admission.frames.len() == 1);
    assert(after.bridge.admission.frames[0].frame_kind == 1);
    assert(after.epm_frames.last().is_none());
    assert(after.accepted_epm_sources.last() == Some(source));
    assert(after.bridge.admission.frames[0].cursor
        == after.bridge.admission.frames[0].region_end);
    assert(after.accepted_epm_sources.last().unwrap().epm.snapshot.bytes
        == product_epm_modeled_empty_snapshot_bytes());
    assert(after.bridge.transcript == rpb3_score_transcript::empty_arena());
    assert(after.bridge.admission.arena.len() == 0);
    assert(score_bridge::bridge_bool_slot(
        after.bridge.admission.frames[0], 1, 4,
    ) == Some(false));
    assert(admission::admission_family_complete(after.bridge.admission.frames[0]));
    assert(admission::admission_slots_complete(after.bridge.admission.frames[0]));
    assert(after.bridge.admission.frames[0].waiting_id.is_none());
    assert(after.bridge.admission.frames[0].score_handle.is_none());
    product_epm_empty_snapshot_selected_nonempty(after);
    assert(product_epm_source_root_score_selected(after).is_some());
    assert(product_source_step(
        after,
        product_epm_source_root_score_selected(after).unwrap(),
        ProductSourceAction::RootEPathMapScore,
    ));
}

pub proof fn product_epm_source_receipt_binds_root_eof(state: ProductScoreState)
    requires
        product_score_wf(state),
        state.bridge.admission.frames.len() == 1,
        state.bridge.admission.frames[0].frame_kind == 1,
        state.accepted_epm_sources[0].is_some(),
    ensures {
        let source = state.accepted_epm_sources[0].unwrap();
        let frame = state.bridge.admission.frames[0];
        let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
        &&& epm_source_accept::epm_source_finish_accept(source).is_some()
        &&& frame.program_type_id == 31
        &&& frame.returns.len() > 0
        &&& frame.returns.last() == accepted.frames.last().returns.last()
        &&& frame.returns.last().body_bytes == source.epm.snapshot.bytes
        &&& match frame.returns.last().kind {
            admission::AdmissionReturnedKind::EPathMapSnapshot {
                mode, entries_canonical_path_stable,
            } => mode == epm_source_accept::epm_mode_code(source.epm.mode)
                && entries_canonical_path_stable == source.epm.entries_stable,
            _ => false,
        }
    },
{
    let source = state.accepted_epm_sources[0].unwrap();
    let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
    let finished = epm_source_accept::epm_end_linked(source);
    assert(epm_source_accept::epm_source_accepts_into_program31(
        source, finished, accepted,
    ));
    epm_source_accept::epm_source_accept_carries_end_summary(
        source, finished, accepted,
    );
    assert(product_epm_source_receipt_matches(
        state.bridge.admission.frames[0], state.indexed_regions[0], source,
    ));
}

pub proof fn product_epm_source_receipt_binds_top_eof(state: ProductScoreState)
    requires
        product_score_wf(state),
        state.bridge.admission.frames.len() > 0,
        state.accepted_epm_sources.last().is_some(),
    ensures {
        let source = state.accepted_epm_sources.last().unwrap();
        let frame = state.bridge.admission.frames.last();
        let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
        &&& epm_source_accept::epm_source_finish_accept(source).is_some()
        &&& frame.family == admission::AdmissionFamily::Other
        &&& frame.program_type_id == 31
        &&& frame.returns.len() > 0
        &&& frame.returns.last() == accepted.frames.last().returns.last()
        &&& frame.returns.last().body_bytes == source.epm.snapshot.bytes
        &&& match frame.returns.last().kind {
            admission::AdmissionReturnedKind::EPathMapSnapshot {
                mode, entries_canonical_path_stable,
            } => mode == epm_source_accept::epm_mode_code(source.epm.mode)
                && entries_canonical_path_stable == source.epm.entries_stable,
            _ => false,
        }
    },
{
    let source = state.accepted_epm_sources.last().unwrap();
    let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
    let finished = epm_source_accept::epm_end_linked(source);
    assert(epm_source_accept::epm_source_accepts_into_program31(
        source, finished, accepted,
    ));
    epm_source_accept::epm_source_accept_carries_end_summary(
        source, finished, accepted,
    );
    assert(product_epm_source_receipt_matches(
        state.bridge.admission.frames.last(), state.indexed_regions.last(), source,
    ));
}

pub open spec fn product_epm_source_bridge_finish_selected(
    before: ProductScoreState,
) -> Option<(score_bridge::ScoreAdmissionBridge, usize, usize, usize, usize)> {
    if !product_score_wf(before)
        || before.bridge.admission.frames.len() == 0
        || before.bridge.admission.frames.last().frame_kind != 1
        || before.bridge.admission.frames.last().family != admission::AdmissionFamily::Other
        || before.bridge.admission.frames.last().program_type_id != 31
        || before.bridge.admission.frames.last().waiting_id.is_some()
        || before.bridge.admission.frames.last().score_handle.is_some()
        || !admission::admission_family_complete(before.bridge.admission.frames.last())
        || before.epm_frames.last().is_some()
        || before.accepted_epm_sources.last().is_none()
    {
        None
    } else {
        let source = before.accepted_epm_sources.last().unwrap();
        match score_bridge::bridge_bool_slot(before.bridge.admission.frames.last(), 1, 4) {
            None => None,
            Some(connective_used) => {
                let flag = if connective_used { 1i64 } else { 0i64 };
                match score_bridge::bridge_append_unattached(
                    before.bridge,
                    rpb3_score_transcript::ScoreNode::BytesAtom(source.epm.snapshot.bytes),
                ) {
                    None => None,
                    Some((after_bytes, bytes_handle)) =>
                        match score_bridge::bridge_append_unattached(
                            after_bytes, rpb3_score_transcript::ScoreNode::Integer(flag),
                        ) {
                            None => None,
                            Some((after_flag, flag_handle)) =>
                                match score_bridge::bridge_constructor_step(
                                    after_flag, 13i32,
                                    seq![bytes_handle, flag_handle],
                                ) {
                                    None => None,
                                    Some((next, constructor_handle, score_handle)) =>
                                        Some((next, bytes_handle, flag_handle,
                                            constructor_handle, score_handle)),
                                },
                        },
                }
            },
        }
    }
}

pub proof fn product_epm_source_bridge_finish_selected_exact(
    before: ProductScoreState,
)
    ensures match product_epm_source_bridge_finish_selected(before) {
        None => true,
        Some((next, bytes_handle, flag_handle, constructor_handle, score_handle)) => {
            let source = before.accepted_epm_sources.last().unwrap();
            let connective_used = score_bridge::bridge_bool_slot(
                before.bridge.admission.frames.last(), 1, 4,
            ).unwrap();
            let flag = if connective_used { 1i64 } else { 0i64 };
            &&& product_epm_source_receipt_matches(
                before.bridge.admission.frames.last(),
                before.indexed_regions.last(), source,
            )
            &&& score_bridge::bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& score_bridge::bridge_score_step_shape(before.bridge, next)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& rpb3_score_transcript::handle_contents(
                next.transcript, bytes_handle,
            ) == Some(rpb3_score_transcript::ScoreNode::BytesAtom(
                source.epm.snapshot.bytes,
            ))
            &&& rpb3_score_transcript::handle_contents(
                next.transcript, flag_handle,
            ) == Some(rpb3_score_transcript::ScoreNode::Integer(flag))
            &&& rpb3_score_transcript::handle_contents(
                next.transcript, score_handle,
            ) == Some(rpb3_score_transcript::ScoreNode::Node(
                seq![constructor_handle, bytes_handle, flag_handle],
            ))
            &&& rpb3_score_transcript::handle_contents(
                next.transcript, constructor_handle,
            ) == Some(rpb3_score_transcript::ScoreNode::Integer(13i64))
        },
    },
{
    if product_epm_source_bridge_finish_selected(before).is_some() {
        let source = before.accepted_epm_sources.last().unwrap();
        let flag = if score_bridge::bridge_bool_slot(
            before.bridge.admission.frames.last(), 1, 4,
        ).unwrap() { 1i64 } else { 0i64 };
        let bytes_kind = rpb3_score_transcript::ScoreNode::BytesAtom(
            source.epm.snapshot.bytes,
        );
        assert(product_epm_source_receipt_matches(
            before.bridge.admission.frames.last(),
            before.indexed_regions.last(), source,
        ));
        assert(admission::admission_state_wf(before.bridge.admission));
        score_bridge::bridge_append_unattached_exact(before.bridge, bytes_kind);
        score_bridge::bridge_append_unattached_preserves_admission_wf(
            before.bridge, bytes_kind,
        );
        score_bridge::bridge_append_unattached_shape(before.bridge, bytes_kind);
        let (after_bytes, bytes_handle) = score_bridge::bridge_append_unattached(
            before.bridge, bytes_kind,
        ).unwrap();
        let flag_kind = rpb3_score_transcript::ScoreNode::Integer(flag);
        score_bridge::bridge_append_unattached_exact(after_bytes, flag_kind);
        score_bridge::bridge_append_unattached_preserves_admission_wf(
            after_bytes, flag_kind,
        );
        score_bridge::bridge_append_unattached_shape(after_bytes, flag_kind);
        let (after_flag, flag_handle) = score_bridge::bridge_append_unattached(
            after_bytes, flag_kind,
        ).unwrap();
        score_bridge::bridge_constructor_exact(
            after_flag, 13i32, seq![bytes_handle, flag_handle],
        );
        score_bridge::bridge_constructor_shape(
            after_flag, 13i32, seq![bytes_handle, flag_handle],
        );
        let (next, constructor_handle, score_handle) =
            score_bridge::bridge_constructor_step(
                after_flag, 13i32, seq![bytes_handle, flag_handle],
            ).unwrap();
        score_bridge::bridge_score_step_shape_transitive(
            before.bridge, after_bytes, after_flag,
        );
        score_bridge::bridge_score_step_shape_transitive(
            before.bridge, after_flag, next,
        );
        assert(constructor_handle + 1 == score_handle);
        assert(rpb3_score_transcript::handle_contents(
            next.transcript, bytes_handle,
        ) == Some(bytes_kind));
        assert(rpb3_score_transcript::handle_contents(
            next.transcript, flag_handle,
        ) == Some(flag_kind));
        assert(seq![constructor_handle] + seq![bytes_handle, flag_handle]
            == seq![constructor_handle, bytes_handle, flag_handle]);
        assert(score_bridge::bridge_well_formed(next));
        assert(admission::admission_state_wf(next.admission));
        assert(score_bridge::bridge_score_step_shape(before.bridge, next));
        assert(next.admission.frames.last().score_handle == Some(score_handle));
        assert(rpb3_score_transcript::handle_contents(
            next.transcript, constructor_handle,
        ) == Some(rpb3_score_transcript::ScoreNode::Integer(13i64)));
        assert(rpb3_score_transcript::handle_contents(
            next.transcript, score_handle,
        ) == Some(rpb3_score_transcript::ScoreNode::Node(
            seq![constructor_handle, bytes_handle, flag_handle],
        )));
    }
}

pub open spec fn product_epm_source_scored_product(
    before: ProductScoreState, next: score_bridge::ScoreAdmissionBridge,
) -> ProductScoreState {
    let candidate = ProductScoreState {
        bridge: next,
        map: map::IntegratedAdmissionState {
            admission: next.admission,
            map_frames: before.map.map_frames,
            transcript: next.transcript,
        },
        epm_frames: before.epm_frames,
        indexed_regions: before.indexed_regions,
        accepted_epm_sources: Seq::empty(),
    };
    ProductScoreState {
        accepted_epm_sources: product_epm_receipts_carried(before, candidate),
        ..candidate
    }
}

pub open spec fn product_epm_source_root_score_selected(
    before: ProductScoreState,
) -> Option<ProductScoreState> {
    if before.bridge.admission.frames.len() != 1 {
        None
    } else { match product_epm_source_bridge_finish_selected(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(product_epm_source_scored_product(before, next)),
    } }
}

pub open spec fn product_epm_source_root_score_step(
    before: ProductScoreState, after: ProductScoreState,
) -> bool {
    product_epm_source_root_score_selected(before) == Some(after)
}

pub open spec fn product_epm_source_child_score_selected(
    before: ProductScoreState,
) -> Option<ProductScoreState> {
    if before.bridge.admission.frames.len() < 2
        || before.map.map_frames.last().is_some()
    {
        None
    } else { match product_epm_source_bridge_finish_selected(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(product_epm_source_scored_product(before, next)),
    } }
}

pub open spec fn product_epm_source_child_score_step(
    before: ProductScoreState, after: ProductScoreState,
) -> bool {
    product_epm_source_child_score_selected(before) == Some(after)
}

pub proof fn product_epm_source_root_score_selected_exact(
    before: ProductScoreState,
)
    ensures match product_epm_source_root_score_selected(before) {
        None => true,
        Some(after) => {
            &&& product_score_wf(after)
            &&& after.bridge.admission.frames.len() == 1
            &&& after.bridge.admission.frames[0].frame_kind == 1
            &&& after.indexed_regions[0].bytes
                == after.bridge.admission.frames[0].source_bytes
            &&& after.bridge.admission.frames[0].score_handle.is_some()
            &&& product_epm_source_root_score_step(before, after)
        },
    },
{
    product_epm_source_bridge_finish_selected_exact(before);
    if product_epm_source_root_score_selected(before).is_some() {
        let (next, _, _, _, _) =
            product_epm_source_bridge_finish_selected(before).unwrap();
        let after = product_epm_source_root_score_selected(before).unwrap();
        assert(after.bridge == next);
        assert(after.accepted_epm_sources
            == product_epm_receipts_carried(before, after));
        product_root_score_shape_preserves_exact(before, after);
    }
}

pub proof fn product_epm_source_child_score_selected_exact(
    before: ProductScoreState,
)
    ensures match product_epm_source_child_score_selected(before) {
        None => true,
        Some(after) => {
            &&& product_score_wf(after)
            &&& after.bridge.admission.frames.len()
                == before.bridge.admission.frames.len()
            &&& after.bridge.admission.frames.last().score_handle.is_some()
            &&& product_epm_source_child_score_step(before, after)
        },
    },
{
    product_epm_source_bridge_finish_selected_exact(before);
    if product_epm_source_child_score_selected(before).is_some() {
        let (next, _, _, _, _) =
            product_epm_source_bridge_finish_selected(before).unwrap();
        let after = product_epm_source_child_score_selected(before).unwrap();
        assert(after.bridge == next);
        assert(after.accepted_epm_sources
            == product_epm_receipts_carried(before, after));
        product_epm_score_preserves_map_wf(before, after);
        assert forall|i: int| 0 <= i < after.epm_frames.len() implies {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                    && frame.snapshot == after.indexed_regions[i]
                    && epm::frame_layout_wf(frame),
            }
        } by {
            assert(after.epm_frames[i] == before.epm_frames[i]);
            assert(after.indexed_regions[i] == before.indexed_regions[i]);
            if i < after.epm_frames.len() - 1 {
                assert(after.bridge.admission.frames[i]
                    == before.bridge.admission.frames[i]);
            } else {
                assert(i == after.epm_frames.len() - 1);
                assert(after.bridge.admission.frames[i].source_bytes
                    == before.bridge.admission.frames[i].source_bytes);
                assert(after.bridge.admission.frames[i].frame_kind
                    == before.bridge.admission.frames[i].frame_kind);
            }
        }
        product_epm_receipts_carried_preserves(before, after);
        assert(product_score_wf(after));
    }
}

pub open spec fn product_epm_source_child_complete_selected(
    before: ProductScoreState,
) -> Option<ProductScoreState> {
    match product_epm_source_child_score_selected(before) {
        None => None,
        Some(scored) => match map::integrated_admission_driver_step(
            scored.map,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ) {
            Some(map::IntegratedAdmissionOutcome::MessageParentAccepted { state: next_map }) => {
                let candidate = ProductScoreState {
                    bridge: score_bridge::ScoreAdmissionBridge {
                        admission: next_map.admission,
                        transcript: next_map.transcript,
                    },
                    map: next_map,
                    epm_frames: scored.epm_frames.drop_last(),
                    indexed_regions: scored.indexed_regions.drop_last(),
                    accepted_epm_sources: Seq::empty(),
                };
                let next = ProductScoreState {
                    accepted_epm_sources: product_epm_receipts_carried(scored, candidate),
                    ..candidate
                };
                if product_map_step(
                    scored, next,
                    map::IntegratedAdmissionAction::Message {
                        action: admission::ConcreteFrameActionModel::Complete,
                    },
                ) { Some(next) } else { None }
            },
            _ => None,
        },
    }
}

pub proof fn product_epm_source_child_complete_selected_exact(
    before: ProductScoreState,
)
    ensures match product_epm_source_child_complete_selected(before) {
        None => true,
        Some(after) => {
            let scored = product_epm_source_child_score_selected(before).unwrap();
            &&& product_score_wf(after)
            &&& product_map_step(
                scored, after,
                map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::Complete,
                },
            )
            &&& after.bridge.admission.frames.len() + 1
                == before.bridge.admission.frames.len()
            &&& after.bridge.transcript == scored.bridge.transcript
        },
    },
{
    if let Some(after) = product_epm_source_child_complete_selected(before) {
        let scored = product_epm_source_child_score_selected(before).unwrap();
        product_epm_source_child_score_selected_exact(before);
        product_map_step_preserves_exact(
            scored, after,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        );
    }
}

pub open spec fn product_source_step(
    before: ProductScoreState, after: ProductScoreState,
    action: ProductSourceAction,
) -> bool {
    product_score_wf(before) && match action {
        ProductSourceAction::Map { action } => product_map_step(before, after, action),
        ProductSourceAction::RootScore => product_root_score_step(before, after),
        ProductSourceAction::RootEPathMapScore =>
            product_epm_source_root_score_step(before, after),
        ProductSourceAction::ChildEPathMapScore =>
            product_epm_source_child_score_step(before, after),
        ProductSourceAction::EpmHandoff { backing, plan, validated } =>
            product_epm_handoff_step(before, after, backing, plan, validated),
        ProductSourceAction::EpmContinue { event } =>
            product_epm_continue_step(before, after, event),
        ProductSourceAction::EpmPush { event, action, child } =>
            product_epm_push_step(before, after, event, action, child),
        ProductSourceAction::EpmScore => product_epm_score_step(before, after),
        ProductSourceAction::EpmChildReturn {
            summary, admission_summary, nested_after,
        } => product_epm_child_return_step(
            before, after, summary, admission_summary, nested_after,
        ),
        ProductSourceAction::EpmComplete { action } =>
            product_epm_complete_step(before, after, action),
        ProductSourceAction::EpmFinishAccept =>
            product_epm_finish_accept_step(before, after),
    }
}

pub proof fn product_root_initial_exact(
    root: admission::AdmissionFrameState,
    backing: epm::IndexedBackingImage,
)
    requires product_root_initial(root, backing).is_some(),
    ensures product_score_wf(product_root_initial(root, backing).unwrap()),
{
    map::integrated_initial_is_well_formed(root);
    score_bridge::bridge_initial_exact(root);
    let state = product_root_initial(root, backing).unwrap();
    product_frame_source_anchors_root(state);
}

pub proof fn product_map_push_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
    outcome: map::IntegratedAdmissionOutcome,
)
    requires
        product_score_wf(before),
        product_map_step(before, after, action),
        map::integrated_admission_driver_step(before.map, action) == Some(outcome),
        match outcome {
            map::IntegratedAdmissionOutcome::MessagePushed { .. }
            | map::IntegratedAdmissionOutcome::MapValuePushed { .. } => true,
            _ => false,
        },
    ensures product_score_wf(after),
{
    map::integrated_driver_preserves_wf(before.map, action);
    assert(map::integrated_admission_wf(after.map));
    assert(score_bridge::bridge_well_formed(after.bridge));
    assert(admission::admission_state_wf(after.bridge.admission));
    let parent = before.bridge.admission.frames.len() - 1;
    assert(after.epm_frames == before.epm_frames.push(None));
    assert(after.epm_frames.len() == after.bridge.admission.frames.len());
    assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
    match action {
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Push { field, .. },
        } => {
            let child = product_indexed_child(before.indexed_regions.last(), field).unwrap();
            assert(after.indexed_regions == before.indexed_regions.push(child));
            assert(child.bytes == field.payload_bytes);
            assert(child.bytes == after.bridge.admission.frames[parent + 1].source_bytes);
            assert(epm::indexed_image_wf(child));
            assert(after.bridge.admission.frames[parent + 1].physical_source
                == product_indexed_start_anchor(child).source);
            assert(after.bridge.admission.frames[parent + 1].physical_start
                == product_indexed_start_anchor(child).offset);
        },
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::PushIndexed { field, .. },
        } => {
            let child = product_indexed_child(before.indexed_regions.last(), field).unwrap();
            assert(after.indexed_regions == before.indexed_regions.push(child));
            assert(child.bytes == field.payload_bytes);
            assert(child.bytes == after.bridge.admission.frames[parent + 1].source_bytes);
            assert(epm::indexed_image_wf(child));
            assert(after.bridge.admission.frames[parent + 1].physical_source
                == product_indexed_start_anchor(child).source);
            assert(after.bridge.admission.frames[parent + 1].physical_start
                == product_indexed_start_anchor(child).offset);
        },
        map::IntegratedAdmissionAction::MapValue { field, .. } => {
            let child = product_indexed_child(before.indexed_regions.last(), field).unwrap();
            assert(after.indexed_regions == before.indexed_regions.push(child));
            assert(child.bytes == field.payload_bytes);
            assert(child.bytes == after.bridge.admission.frames[parent + 1].source_bytes);
            assert(epm::indexed_image_wf(child));
            assert(after.bridge.admission.frames[parent + 1].physical_source
                == product_indexed_start_anchor(child).source);
            assert(after.bridge.admission.frames[parent + 1].physical_start
                == product_indexed_start_anchor(child).offset);
        },
        map::IntegratedAdmissionAction::MapValueIndexed { field, .. } => {
            let child = product_indexed_child(before.indexed_regions.last(), field).unwrap();
            assert(after.indexed_regions == before.indexed_regions.push(child));
            assert(child.bytes == field.payload_bytes);
            assert(child.bytes == after.bridge.admission.frames[parent + 1].source_bytes);
            assert(epm::indexed_image_wf(child));
            assert(after.bridge.admission.frames[parent + 1].physical_source
                == product_indexed_start_anchor(child).source);
            assert(after.bridge.admission.frames[parent + 1].physical_start
                == product_indexed_start_anchor(child).offset);
        },
        _ => assert(false),
    }
    assert(after.indexed_regions.drop_last() == before.indexed_regions);
    product_frame_source_anchors_push(before, after);
    product_epm_receipts_carried_preserves(before, after);
    assert forall|i: int| 0 <= i < after.epm_frames.len() ==> {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(frame) => {
                &&& after.bridge.admission.frames[i].frame_kind == 3
                &&& frame.snapshot == after.indexed_regions[i]
                &&& epm::frame_layout_wf(frame)
            },
        }
    } by {
        if 0 <= i && i < after.epm_frames.len() {
            if i <= parent {
                assert(i < before.epm_frames.len());
                assert(i < before.indexed_regions.len());
                assert(after.indexed_regions.len() == before.indexed_regions.len() + 1);
                assert(after.indexed_regions.len() > 0);
                assert(i < after.indexed_regions.len() as int - 1);
                assert(after.epm_frames == before.epm_frames.push(None));
                assert(after.indexed_regions.drop_last() == before.indexed_regions);
                vstd::seq::lemma_seq_push_index_different_alt(
                    before.epm_frames, None, i,
                );
                vstd::seq::lemma_seq_subrange_index_alt(
                    after.indexed_regions, 0,
                    after.indexed_regions.len() as int - 1, i,
                );
                assert(after.epm_frames[i] == before.epm_frames[i]);
                assert(after.indexed_regions[i] == before.indexed_regions[i]);
                if i < parent {
                    assert(after.bridge.admission.frames[i]
                        == before.bridge.admission.frames[i]);
                } else {
                    assert(i == parent);
                    assert(after.bridge.admission.frames[i].source_bytes
                        == before.bridge.admission.frames[i].source_bytes);
                    assert(after.bridge.admission.frames[i].physical_source
                        == before.bridge.admission.frames[i].physical_source);
                    assert(after.bridge.admission.frames[i].physical_start
                        == before.bridge.admission.frames[i].physical_start);
                    assert(after.bridge.admission.frames[i].frame_kind
                        == before.bridge.admission.frames[i].frame_kind);
                }
            } else {
                assert(i == parent + 1);
                assert(after.epm_frames[i].is_none());
            }
        }
    }
    assert(forall|i: int| 0 <= i < after.epm_frames.len() ==> {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(frame) => {
                &&& after.bridge.admission.frames[i].frame_kind == 3
                &&& frame.snapshot == after.indexed_regions[i]
                &&& epm::frame_layout_wf(frame)
            },
        }
    });
    product_score_wf_intro(after);
}

pub proof fn product_map_pop_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
    outcome: map::IntegratedAdmissionOutcome,
)
    requires
        product_score_wf(before),
        product_map_step(before, after, action),
        map::integrated_admission_driver_step(before.map, action) == Some(outcome),
        match outcome {
            map::IntegratedAdmissionOutcome::MessageParentAccepted { .. }
            | map::IntegratedAdmissionOutcome::MapValueAccepted { .. }
            | map::IntegratedAdmissionOutcome::MapEntryCompleted { .. } => true,
            _ => false,
        },
    ensures product_score_wf(after),
{
    map::integrated_driver_preserves_wf(before.map, action);
    assert(map::integrated_admission_wf(after.map));
    assert(score_bridge::bridge_well_formed(after.bridge));
    assert(admission::admission_state_wf(after.bridge.admission));
    assert(after.epm_frames == before.epm_frames.drop_last());
    assert(after.indexed_regions == before.indexed_regions.drop_last());
    assert(after.bridge.admission.frames.len() + 1
        == before.bridge.admission.frames.len());
    match action {
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        } => {
            admission::concrete_message_complete_child_simulates(
                before.map.admission,
            );
        },
        map::IntegratedAdmissionAction::MapAccept => {
            if before.map.map_frames.len() > 1 {
                if let Some(frame) = before.map.map_frames[
                    before.map.map_frames.len() - 2
                ] {
                    map::map_admission_value_accept_pop_source_anchor_exact(
                        frame, before.map.admission,
                    );
                }
            }
        },
        map::IntegratedAdmissionAction::MapFinish => {
            if before.map.map_frames.len() > 0 {
                if let Some(frame) = before.map.map_frames.last() {
                    map::map_admission_entry_complete_source_anchor_exact(
                        frame, before.map.admission,
                    );
                }
            }
        },
        _ => {},
    }
    assert forall|i: int| 0 <= i < after.epm_frames.len() implies {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                && frame.snapshot == after.indexed_regions[i]
                && epm::frame_layout_wf(frame),
        }
    } by {
        assert(after.epm_frames[i] == before.epm_frames[i]);
        assert(after.indexed_regions[i] == before.indexed_regions[i]);
        if i < after.epm_frames.len() - 1 {
            assert(after.bridge.admission.frames[i]
                == before.bridge.admission.frames[i]);
        } else {
            assert(i == after.epm_frames.len() - 1);
            assert(after.epm_frames[i].is_none());
            assert(after.bridge.admission.frames[i].source_bytes
                == before.bridge.admission.frames[i].source_bytes);
            assert(after.bridge.admission.frames[i].physical_source
                == before.bridge.admission.frames[i].physical_source);
            assert(after.bridge.admission.frames[i].physical_start
                == before.bridge.admission.frames[i].physical_start);
        }
    }
    product_frame_source_anchors_pop(before, after);
    product_epm_receipts_carried_preserves(before, after);
    assert(after.map.admission == after.bridge.admission);
    assert(after.map.transcript == after.bridge.transcript);
    assert(after.epm_frames.len() == after.bridge.admission.frames.len());
    assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
    assert(forall|i: int| 0 <= i < after.epm_frames.len() ==> {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                && frame.snapshot == after.indexed_regions[i]
                && epm::frame_layout_wf(frame),
        }
    });
    product_score_wf_intro(after);
}

pub proof fn product_map_steady_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
    outcome: map::IntegratedAdmissionOutcome,
)
    requires
        product_score_wf(before),
        product_map_step(before, after, action),
        map::integrated_admission_driver_step(before.map, action) == Some(outcome),
        match outcome {
            map::IntegratedAdmissionOutcome::MessageContinued { .. }
            | map::IntegratedAdmissionOutcome::MapKeyContinued { .. }
            | map::IntegratedAdmissionOutcome::MapParScored { .. } => true,
            _ => false,
        },
    ensures product_score_wf(after),
{
    map::integrated_driver_preserves_wf(before.map, action);
    assert(map::integrated_admission_wf(after.map));
    assert(score_bridge::bridge_well_formed(after.bridge));
    assert(admission::admission_state_wf(after.bridge.admission));
    assert(after.epm_frames == before.epm_frames);
    assert(after.indexed_regions == before.indexed_regions);
    assert(after.bridge.admission.frames.len()
        == before.bridge.admission.frames.len());
    assert forall|i: int| 0 <= i < after.epm_frames.len() implies {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                && frame.snapshot == after.indexed_regions[i]
                && epm::frame_layout_wf(frame),
        }
    } by {
        assert(after.epm_frames[i] == before.epm_frames[i]);
        assert(after.indexed_regions[i] == before.indexed_regions[i]);
        if i < after.epm_frames.len() - 1 {
            assert(after.bridge.admission.frames[i]
                == before.bridge.admission.frames[i]);
        } else {
            assert(i == after.epm_frames.len() - 1);
            assert(after.bridge.admission.frames[i].source_bytes
                == before.bridge.admission.frames[i].source_bytes);
            assert(after.bridge.admission.frames[i].physical_source
                == before.bridge.admission.frames[i].physical_source);
            assert(after.bridge.admission.frames[i].physical_start
                == before.bridge.admission.frames[i].physical_start);
            assert(after.bridge.admission.frames[i].frame_kind
                == before.bridge.admission.frames[i].frame_kind);
        }
    }
    assert(product_frame_source_anchors_match(after));
    assert(after.map.admission == after.bridge.admission);
    assert(after.map.transcript == after.bridge.transcript);
    assert(after.epm_frames.len() == after.bridge.admission.frames.len());
    assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
    assert(forall|i: int| 0 <= i < after.epm_frames.len() ==> {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                && frame.snapshot == after.indexed_regions[i]
                && epm::frame_layout_wf(frame),
        }
    });
    product_epm_receipts_carried_preserves(before, after);
    assert(product_score_wf(after));
}

pub proof fn product_map_step_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
)
    requires product_score_wf(before), product_map_step(before, after, action),
    ensures product_score_wf(after),
{
    match map::integrated_admission_driver_step(before.map, action) {
        None => assert(false),
        Some(outcome) => match outcome {
            map::IntegratedAdmissionOutcome::MessagePushed { .. }
            | map::IntegratedAdmissionOutcome::MapValuePushed { .. } =>
                product_map_push_preserves_exact(before, after, action, outcome),
            map::IntegratedAdmissionOutcome::MessageParentAccepted { .. }
            | map::IntegratedAdmissionOutcome::MapValueAccepted { .. }
            | map::IntegratedAdmissionOutcome::MapEntryCompleted { .. } =>
                product_map_pop_preserves_exact(before, after, action, outcome),
            map::IntegratedAdmissionOutcome::MessageContinued { .. }
            | map::IntegratedAdmissionOutcome::MapKeyContinued { .. }
            | map::IntegratedAdmissionOutcome::MapParScored { .. } =>
                product_map_steady_preserves_exact(before, after, action, outcome),
            map::IntegratedAdmissionOutcome::RootFinished { .. } => assert(false),
        },
    }
}

pub proof fn product_epm_handoff_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
)
    requires product_score_wf(before),
        product_epm_handoff_step(before, after, backing, plan, validated),
    ensures product_score_wf(after),
{
    if let Some(linked) = epm::linked_map_epm_handoff(
        before.bridge.admission, before.indexed_regions, backing, plan, validated,
    ) {
        assert(epm::linked_indexed_sources(linked));
        assert(epm::frame_layout_wf(linked.epm));
    }
    product_epm_receipts_carried_preserves(before, after);
    assert(product_score_wf(after));
}

pub proof fn product_epm_continue_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
    event: epm::EpmEvent,
)
    requires product_score_wf(before), product_epm_continue_step(before, after, event),
    ensures product_score_wf(after),
{
    if let (Some(prior), Some(next)) = (
        product_linked_top(before), product_linked_top(after),
    ) {
        epm::linked_epm_continue_preserves_source_and_progress(prior, next, event);
        assert(after.bridge.admission.arena == before.bridge.admission.arena);
        unchanged_admission_arena_preserves_exact(before.bridge, after.bridge);
        assert(epm::linked_indexed_sources(next));
        assert(epm::frame_layout_wf(next.epm));
        let last = before.bridge.admission.frames.len() - 1;
        assert(before.map.map_frames[last].is_none());
        assert(after.map.map_frames == before.map.map_frames);
        assert(after.map.admission.frames.len() == before.map.admission.frames.len());
        assert(after.epm_frames == before.epm_frames.update(last, Some(next.epm)));
        assert(after.epm_frames.len() == before.epm_frames.len());
        assert(after.epm_frames.len() == last + 1);
        assert(after.map.admission.frames[last].frame_kind == 3);
        assert forall|i: int| 0 <= i < last
            implies after.map.admission.frames[i] == before.map.admission.frames[i] by {}
        assert(map::integrated_admission_wf(after.map));
        assert forall|i: int| 0 <= i < after.epm_frames.len() implies {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => {
                    &&& after.bridge.admission.frames[i].frame_kind == 3
                    &&& frame.snapshot == after.indexed_regions[i]
                    &&& epm::frame_layout_wf(frame)
                },
            }
        } by {
            if i < last {
                assert(i != last);
                assert(after.epm_frames[i] == before.epm_frames[i]);
                assert(after.indexed_regions[i] == before.indexed_regions[i]);
                assert(after.bridge.admission.frames[i]
                    == before.bridge.admission.frames[i]);
            } else {
                assert(i == last);
                assert(after.epm_frames[i] == Some(next.epm));
                assert(after.indexed_regions[i] == next.indexed_regions[i]);
                assert(after.bridge.admission.frames[i] == next.admission.frames[i]);
            }
        }
    }
    assert(score_bridge::bridge_well_formed(after.bridge));
    assert(map::integrated_admission_wf(after.map));
    assert(after.map.admission == after.bridge.admission);
    assert(after.map.transcript == after.bridge.transcript);
    assert(after.epm_frames.len() == after.bridge.admission.frames.len());
    assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
    assert(forall|i: int| 0 <= i < after.epm_frames.len() ==> {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                && frame.snapshot == after.indexed_regions[i]
                && epm::frame_layout_wf(frame),
        }
    });
    product_epm_receipts_carried_preserves(before, after);
    assert(product_score_wf(after));
}

pub proof fn product_epm_push_preserves_map_wf(
    before: ProductScoreState, after: ProductScoreState,
    event: epm::EpmEvent, action: epm::EpmAction,
    child: admission::AdmissionFrameState,
)
    requires product_score_wf(before),
        product_epm_push_step(before, after, event, action, child),
    ensures
        score_bridge::bridge_well_formed(after.bridge),
        map::integrated_admission_wf(after.map),
{
    if let (Some(prior), Some(next)) = (
        product_linked_top(before), product_linked_parent(after),
    ) {
        epm::linked_epm_push_preserves_parent_progress(
            prior, next, event, action, child,
        );
        assert(after.bridge.admission.arena == before.bridge.admission.arena);
        unchanged_admission_arena_preserves_exact(before.bridge, after.bridge);
        assert(epm::linked_indexed_sources(next));
        assert(epm::frame_layout_wf(next.epm));
        let parent_index = before.bridge.admission.frames.len() - 1;
        let child_index = parent_index + 1;
        assert(before.map.map_frames[parent_index].is_none());
        assert(after.map.map_frames == before.map.map_frames.push(None));
        assert(after.map.admission.frames.len() == before.map.admission.frames.len() + 1);
        assert(after.map.admission.frames[parent_index].frame_kind == 3);
        assert(after.map.admission.frames[child_index].frame_kind != 2);
        assert forall|i: int| 0 <= i < parent_index
            implies after.map.admission.frames[i] == before.map.admission.frames[i] by {}
        assert(map::integrated_admission_wf(after.map));
    } else {
        assert(false);
    }
}

pub proof fn product_epm_push_preserves_indexed(
    before: ProductScoreState, after: ProductScoreState,
    event: epm::EpmEvent, action: epm::EpmAction,
    child: admission::AdmissionFrameState,
)
    requires product_score_wf(before),
        product_epm_push_step(before, after, event, action, child),
    ensures
        after.epm_frames.len() == after.bridge.admission.frames.len(),
        after.indexed_regions.len() == after.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < after.epm_frames.len() ==> {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                    && frame.snapshot == after.indexed_regions[i]
                    && epm::frame_layout_wf(frame),
            }
        },
{
    if let (Some(prior), Some(next)) = (
        product_linked_top(before), product_linked_parent(after),
    ) {
        assert(epm::linked_indexed_sources(next));
        assert(epm::frame_layout_wf(next.epm));
        let parent_index = before.bridge.admission.frames.len() - 1;
        let child_index = parent_index + 1;
        assert(after.epm_frames.len() == after.bridge.admission.frames.len());
        assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
        assert forall|i: int|
            #![trigger after.indexed_regions[i]]
            #![trigger after.epm_frames[i]]
            #![trigger after.bridge.admission.frames[i]]
            0 <= i < after.epm_frames.len() implies {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                    && frame.snapshot == after.indexed_regions[i]
                    && epm::frame_layout_wf(frame),
            }
        } by {
            if i < parent_index {
                assert(after.epm_frames[i] == before.epm_frames[i]);
                assert(after.indexed_regions[i] == before.indexed_regions[i]);
                assert(after.bridge.admission.frames[i]
                    == before.bridge.admission.frames[i]);
            } else if i == parent_index {
                assert(after.epm_frames[i] == Some(next.epm));
                assert(after.indexed_regions[i] == next.indexed_regions[i]);
                assert(after.bridge.admission.frames[i] == next.admission.frames[i]);
            } else {
                assert(i == child_index);
                assert(after.epm_frames[i].is_none());
                assert(after.indexed_regions[i] == next.indexed_regions[i]);
                assert(after.bridge.admission.frames[i] == next.admission.frames[i]);
            }
        }
        assert(forall|i: int| 0 <= i < after.epm_frames.len() ==> {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                    && frame.snapshot == after.indexed_regions[i]
                    && epm::frame_layout_wf(frame),
            }
        });
    } else {
        assert(false);
    }
}

pub proof fn product_epm_push_preserves_source_anchors(
    before: ProductScoreState, after: ProductScoreState,
    event: epm::EpmEvent, action: epm::EpmAction,
    child: admission::AdmissionFrameState,
)
    requires product_score_wf(before),
        product_epm_push_step(before, after, event, action, child),
    ensures product_frame_source_anchors_match(after),
{
    if let (Some(prior), Some(next)) = (
        product_linked_top(before), product_linked_parent(after),
    ) {
        assert(epm::linked_epm_push(prior, next, event, action, child));
        assert(epm::linked_indexed_sources(next));
        assert(after.bridge.admission == next.admission);
        assert(after.indexed_regions == next.indexed_regions);
        assert forall|i: int| 0 <= i < after.indexed_regions.len() implies {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.bridge.admission.frames[i].source_bytes
                == after.indexed_regions[i].bytes
            &&& after.bridge.admission.frames[i].physical_start
                == product_indexed_start_anchor(after.indexed_regions[i]).offset
            &&& after.bridge.admission.frames[i].physical_source
                == product_indexed_start_anchor(after.indexed_regions[i]).source
        } by {
            assert(epm::indexed_position_at(after.indexed_regions[i], 0)
                == product_indexed_start_anchor(after.indexed_regions[i]));
        }
    } else {
        assert(false);
    }
}

pub proof fn product_epm_push_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
    event: epm::EpmEvent, action: epm::EpmAction,
    child: admission::AdmissionFrameState,
)
    requires product_score_wf(before),
        product_epm_push_step(before, after, event, action, child),
    ensures product_score_wf(after),
{
    product_epm_push_preserves_map_wf(before, after, event, action, child);
    product_epm_push_preserves_indexed(before, after, event, action, child);
    assert(after.map.admission == after.bridge.admission);
    assert(after.map.transcript == after.bridge.transcript);
    product_epm_push_preserves_source_anchors(before, after, event, action, child);
    product_epm_receipts_carried_preserves(before, after);
    product_score_wf_intro(after);
}

pub proof fn product_epm_score_preserves_map_wf(
    before: ProductScoreState, after: ProductScoreState,
)
    requires
        product_score_wf(before),
        before.bridge.admission.frames.len() > 0,
        before.bridge.admission.frames.last().frame_kind == 1,
        before.map.map_frames.last().is_none(),
        after.map.map_frames == before.map.map_frames,
        after.map.admission == after.bridge.admission,
        after.map.transcript == after.bridge.transcript,
        score_bridge::bridge_score_step_shape(before.bridge, after.bridge),
        score_bridge::bridge_well_formed(after.bridge),
        admission::admission_state_wf(after.bridge.admission),
    ensures map::integrated_admission_wf(after.map),
{
    let last = before.bridge.admission.frames.len() - 1;
    assert(after.map.map_frames == before.map.map_frames);
    assert(after.map.map_frames[last].is_none());
    assert(after.map.admission == after.bridge.admission);
    assert(after.map.transcript == after.bridge.transcript);
    assert forall|i: int| 0 <= i < after.map.map_frames.len() implies {
        match #[trigger] after.map.map_frames[i] {
            None => after.map.admission.frames[i].frame_kind != 2,
            Some(frame) => after.map.admission.frames[i].frame_kind == 2
                && i > 0
                && after.map.admission.frames[i - 1].family
                    == admission::AdmissionFamily::New
                && after.map.admission.frames[i - 1].program_type_id
                    == frame.owner_type_id
                && after.map.admission.frames[i - 1].waiting_slot == Some(3)
                && after.map.admission.frames[i - 1].waiting_tag
                    == Some(frame.outer_field)
                && after.map.admission.frames[i - 1].waiting_kind == 2
                && after.map.admission.frames[i - 1].waiting_id
                    == after.map.admission.frames[i].return_id
                && after.map.admission.frames[i].return_id.is_some()
                && after.map.admission.frames[i].body_id as int
                    == after.map.admission.frames[i].return_id.unwrap() as int + 1
                && map::map_frame_wf(frame)
                && frame.accepted_arena.len() <= after.map.admission.arena.len()
                && frame.accepted_arena == after.map.admission.arena.subrange(
                    0, frame.accepted_arena.len() as int,
                )
                && map::map_aux_projection(frame, after.map.admission.frames[i]),
        }
    } by {
        if i < last {
            assert(after.map.admission.frames[i]
                == before.map.admission.frames[i]);
            match before.map.map_frames[i] {
                Some(frame) => {
                    assert(frame.accepted_arena.len()
                        <= before.map.admission.arena.len());
                    assert(frame.accepted_arena
                        == before.map.admission.arena.subrange(
                            0, frame.accepted_arena.len() as int,
                        ));
                    assert(after.map.admission.arena.subrange(
                        0, frame.accepted_arena.len() as int,
                    ) == before.map.admission.arena.subrange(
                        0, frame.accepted_arena.len() as int,
                    ));
                    if i > 0 {
                        assert(after.map.admission.frames[i - 1]
                            == before.map.admission.frames[i - 1]);
                    }
                },
                None => {},
            }
        } else {
            assert(i == last);
            assert(after.map.map_frames[i].is_none());
            assert(after.map.admission.frames[i].frame_kind == 1);
        }
    }
}

pub proof fn product_epm_score_indexed_frames_preserved(
    before: ProductScoreState, after: ProductScoreState,
)
    requires
        product_score_wf(before),
        score_bridge::bridge_score_step_shape(before.bridge, after.bridge),
        after.epm_frames == before.epm_frames,
        after.indexed_regions == before.indexed_regions,
    ensures
        after.epm_frames.len() == after.bridge.admission.frames.len(),
        after.indexed_regions.len() == after.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < after.epm_frames.len() ==> {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                    && frame.snapshot == after.indexed_regions[i]
                    && epm::frame_layout_wf(frame),
            }
        },
{
    assert(after.epm_frames.len() == after.bridge.admission.frames.len());
    assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
    assert forall|i: int| 0 <= i < after.epm_frames.len() implies {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                && frame.snapshot == after.indexed_regions[i]
                && epm::frame_layout_wf(frame),
        }
    } by {
        assert(after.epm_frames[i] == before.epm_frames[i]);
        assert(after.indexed_regions[i] == before.indexed_regions[i]);
        if i < after.epm_frames.len() - 1 {
            assert(after.bridge.admission.frames[i]
                == before.bridge.admission.frames[i]);
        } else {
            assert(i == after.epm_frames.len() - 1);
            assert(after.bridge.admission.frames[i].source_bytes
                == before.bridge.admission.frames[i].source_bytes);
            assert(after.bridge.admission.frames[i].frame_kind
                == before.bridge.admission.frames[i].frame_kind);
        }
    }
}

pub proof fn product_epm_score_source_anchors_preserved(
    before: ProductScoreState, after: ProductScoreState,
)
    requires
        product_frame_source_anchors_match(before),
        score_bridge::bridge_score_step_shape(before.bridge, after.bridge),
        after.indexed_regions == before.indexed_regions,
    ensures product_frame_source_anchors_match(after),
{
    assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
    assert forall|i: int| 0 <= i < after.indexed_regions.len() implies {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.bridge.admission.frames[i].source_bytes
            == after.indexed_regions[i].bytes
        &&& after.bridge.admission.frames[i].physical_start
            == product_indexed_start_anchor(after.indexed_regions[i]).offset
        &&& after.bridge.admission.frames[i].physical_source
            == product_indexed_start_anchor(after.indexed_regions[i]).source
    } by {
        assert(after.indexed_regions[i] == before.indexed_regions[i]);
        if i < after.indexed_regions.len() - 1 {
            assert(after.bridge.admission.frames[i]
                == before.bridge.admission.frames[i]);
        } else {
            assert(i == after.indexed_regions.len() - 1);
            assert(after.bridge.admission.frames[i].source_bytes
                == before.bridge.admission.frames[i].source_bytes);
            assert(after.bridge.admission.frames[i].physical_source
                == before.bridge.admission.frames[i].physical_source);
            assert(after.bridge.admission.frames[i].physical_start
                == before.bridge.admission.frames[i].physical_start);
        }
    }
}

pub proof fn product_epm_score_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
)
    requires product_score_wf(before), product_epm_score_step(before, after),
    ensures product_score_wf(after),
{
    score_bridge::bridge_score_source_step_preserves_exact(
        before.bridge, after.bridge,
    );
    score_bridge::bridge_score_source_step_preserves_shape(
        before.bridge, after.bridge,
    );
    product_epm_score_preserves_map_wf(before, after);
    product_epm_score_indexed_frames_preserved(before, after);
    product_epm_score_source_anchors_preserved(before, after);
    product_epm_receipts_carried_preserves(before, after);
    product_score_wf_intro(after);
}

pub proof fn product_message_child_score_frame_image_preserved(
    before: ProductScoreState, after: ProductScoreState,
)
    requires
        product_score_wf(before),
        score_bridge::bridge_score_step_shape(before.bridge, after.bridge),
        after.epm_frames == before.epm_frames,
        after.indexed_regions == before.indexed_regions,
    ensures
        after.epm_frames.len() == after.bridge.admission.frames.len(),
        after.indexed_regions.len() == after.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < after.epm_frames.len() ==> {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                    && frame.snapshot == after.indexed_regions[i]
                    && epm::frame_layout_wf(frame),
            }
        },
        product_frame_source_anchors_match(after),
        after.bridge.admission.frames.len() == before.bridge.admission.frames.len(),
        after.bridge.admission.frames.last().frame_kind
            == before.bridge.admission.frames.last().frame_kind,
{
    product_epm_score_indexed_frames_preserved(before, after);
    product_epm_score_source_anchors_preserved(before, after);
}

pub proof fn product_message_child_score_product_wf(
    before: ProductScoreState, after: ProductScoreState,
)
    requires
        product_message_child_score_step(before, after),
        score_bridge::bridge_well_formed(after.bridge),
        map::integrated_admission_wf(after.map),
        score_bridge::bridge_score_step_shape(before.bridge, after.bridge),
    ensures
        product_score_wf(after),
        after.bridge.admission.frames.len()
            == before.bridge.admission.frames.len(),
        after.bridge.admission.frames.last().frame_kind == 1,
{
    product_message_child_score_frame_image_preserved(before, after);
    product_epm_receipts_carried_preserves(before, after);
    product_score_wf_intro(after);
}

pub proof fn product_message_child_score_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
)
    requires product_message_child_score_step(before, after),
    ensures
        product_score_wf(after),
        after.bridge.admission.frames.len()
            == before.bridge.admission.frames.len(),
        after.bridge.admission.frames.last().frame_kind == 1,
{
    score_bridge::bridge_score_source_step_preserves_exact(
        before.bridge, after.bridge,
    );
    score_bridge::bridge_score_source_step_preserves_shape(
        before.bridge, after.bridge,
    );
    product_epm_score_preserves_map_wf(before, after);
    product_message_child_score_product_wf(before, after);
}

pub proof fn product_epm_child_return_preserves_map_wf(
    before: ProductScoreState, after: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
)
    requires product_score_wf(before), product_epm_child_return_step(
        before, after, summary, admission_summary, nested_after,
    ),
    ensures
        score_bridge::bridge_well_formed(after.bridge),
        map::integrated_admission_wf(after.map),
{
    if let (Some(prior), Some(next)) = (
        product_linked_parent(before), product_linked_top(after),
    ) {
        assert(epm::linked_epm_child_return(
            prior, next, summary, admission_summary,
            before.epm_frames.last(), nested_after,
        ));
        assert(after.bridge.admission.arena == before.bridge.admission.arena);
        unchanged_admission_arena_preserves_exact(before.bridge, after.bridge);
        assert(epm::linked_indexed_sources(next));
        assert(epm::frame_layout_wf(next.epm));
        let last = after.bridge.admission.frames.len() - 1;
        assert(before.map.map_frames[last].is_none());
        assert(after.map.map_frames == before.map.map_frames.drop_last());
        assert(after.map.admission.frames.len() == before.map.admission.frames.len() - 1);
        assert(after.epm_frames == before.epm_frames.drop_last().update(
            before.epm_frames.len() - 2, Some(next.epm),
        ));
        assert(after.epm_frames.len() == after.bridge.admission.frames.len());
        assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
        assert(after.map.admission.frames[last].frame_kind == 3);
        assert forall|i: int| 0 <= i < last
            implies after.map.admission.frames[i] == before.map.admission.frames[i] by {}
        assert(map::integrated_admission_wf(after.map));
    } else {
        assert(false);
    }
}

pub proof fn product_epm_child_return_preserves_indexed(
    before: ProductScoreState, after: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
)
    requires product_score_wf(before), product_epm_child_return_step(
        before, after, summary, admission_summary, nested_after,
    ),
    ensures
        after.epm_frames.len() == after.bridge.admission.frames.len(),
        after.indexed_regions.len() == after.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < after.epm_frames.len() ==> {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => {
                    &&& after.bridge.admission.frames[i].frame_kind == 3
                    &&& frame.snapshot == after.indexed_regions[i]
                    &&& epm::frame_layout_wf(frame)
                },
            }
        },
{
    if let (Some(prior), Some(next)) = (
        product_linked_parent(before), product_linked_top(after),
    ) {
        assert(epm::linked_indexed_sources(next));
        assert(epm::frame_layout_wf(next.epm));
        assert(after.indexed_regions == next.indexed_regions);
        assert(after.bridge.admission == next.admission);
        assert(after.indexed_regions == before.indexed_regions.drop_last());
        let last = after.bridge.admission.frames.len() - 1;
        assert(after.epm_frames == before.epm_frames.drop_last().update(
            before.epm_frames.len() - 2, Some(next.epm),
        ));
        assert(after.epm_frames.len() == after.bridge.admission.frames.len());
        assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
        assert forall|i: int| 0 <= i < after.epm_frames.len() implies {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                    && frame.snapshot == after.indexed_regions[i]
                    && epm::frame_layout_wf(frame),
            }
        } by {
            if i < last {
                assert(after.epm_frames[i] == before.epm_frames[i]);
                assert(after.indexed_regions[i] == before.indexed_regions[i]);
                assert(after.bridge.admission.frames[i]
                    == before.bridge.admission.frames[i]);
            } else {
                assert(i == last);
                assert(after.epm_frames[i] == Some(next.epm));
                assert(after.indexed_regions[i] == next.indexed_regions[i]);
                assert(after.bridge.admission.frames[i] == next.admission.frames[i]);
                assert(next.epm.snapshot == prior.epm.snapshot);
                assert(prior.epm.snapshot == before.indexed_regions[i]);
                assert(next.epm.snapshot == after.indexed_regions[i]);
                assert(epm::indexed_image_wf(next.indexed_regions[i]));
                assert(next.indexed_regions[i].bytes
                    == next.admission.frames[i].source_bytes);
                assert(next.admission.frames[i].frame_kind == 3);
            }
        }
    } else {
        assert(false);
    }
    assert forall|i: int| 0 <= i < after.epm_frames.len() implies {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(frame) => {
                &&& after.bridge.admission.frames[i].frame_kind == 3
                &&& frame.snapshot == after.indexed_regions[i]
                &&& epm::frame_layout_wf(frame)
            },
        }
    } by {
        assert(epm::indexed_image_wf(after.indexed_regions[i]));
        assert(after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes);
        match after.epm_frames[i] {
            None => {},
            Some(frame) => {
                assert(after.bridge.admission.frames[i].frame_kind == 3);
                assert(frame.snapshot == after.indexed_regions[i]);
                assert(epm::frame_layout_wf(frame));
            },
        }
    }
}

pub proof fn product_epm_child_return_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
)
    requires product_score_wf(before), product_epm_child_return_step(
        before, after, summary, admission_summary, nested_after,
    ),
    ensures product_score_wf(after),
{
    product_epm_child_return_preserves_map_wf(
        before, after, summary, admission_summary, nested_after,
    );
    product_epm_child_return_preserves_indexed(
        before, after, summary, admission_summary, nested_after,
    );
    assert(score_bridge::bridge_well_formed(after.bridge));
    assert(map::integrated_admission_wf(after.map));
    assert(after.map.admission == after.bridge.admission);
    assert(after.map.transcript == after.bridge.transcript);
    assert(after.epm_frames.len() == after.bridge.admission.frames.len());
    assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
    product_frame_source_anchors_pop(before, after);
    product_epm_receipts_carried_preserves(before, after);
    assert(forall|i: int| 0 <= i < after.epm_frames.len() ==> {
        &&& epm::indexed_image_wf(after.indexed_regions[i])
        &&& after.indexed_regions[i].bytes
            == after.bridge.admission.frames[i].source_bytes
        &&& match after.epm_frames[i] {
            None => true,
            Some(frame) => {
                &&& after.bridge.admission.frames[i].frame_kind == 3
                &&& frame.snapshot == after.indexed_regions[i]
                &&& epm::frame_layout_wf(frame)
            },
        }
    });
    product_score_wf_intro(after);
}

pub proof fn product_epm_complete_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
    action: epm::EpmAction,
)
    requires product_score_wf(before), product_epm_complete_step(before, after, action),
    ensures product_score_wf(after),
{
    if let (Some(prior), Some(next)) = (
        product_linked_top(before), product_linked_top(after),
    ) {
        assert(epm::linked_epm_complete(prior, next, action));
        assert(score_bridge::bridge_same(before.bridge, after.bridge));
        assert(after.bridge.admission == before.bridge.admission);
        assert(next.admission == before.bridge.admission);
        assert(next.indexed_regions == before.indexed_regions);
        assert(epm::linked_indexed_sources(next));
        assert(epm::frame_layout_wf(next.epm));
        assert(after.map == before.map);
        assert(after.indexed_regions == before.indexed_regions);
        let last = before.epm_frames.len() - 1;
        assert(after.epm_frames == before.epm_frames.update(last, Some(next.epm)));
        assert(after.epm_frames.len() == after.bridge.admission.frames.len());
        assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
        assert forall|i: int| 0 <= i < after.epm_frames.len() implies {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => after.bridge.admission.frames[i].frame_kind == 3
                    && frame.snapshot == after.indexed_regions[i]
                    && epm::frame_layout_wf(frame),
            }
        } by {
            if i < last {
                assert(after.epm_frames[i] == before.epm_frames[i]);
                assert(after.indexed_regions[i] == before.indexed_regions[i]);
                assert(after.bridge.admission.frames[i]
                    == before.bridge.admission.frames[i]);
                assert(epm::indexed_image_wf(after.indexed_regions[i]));
                assert(after.indexed_regions[i].bytes
                    == after.bridge.admission.frames[i].source_bytes);
            } else {
                assert(i == last);
                assert(after.epm_frames[i] == Some(next.epm));
                assert(after.indexed_regions[i] == next.indexed_regions[i]);
                assert(after.bridge.admission.frames[i] == next.admission.frames[i]);
                assert(next.epm.snapshot == after.indexed_regions[i]);
                assert(epm::indexed_image_wf(next.indexed_regions[i]));
                assert(next.indexed_regions[i].bytes
                    == next.admission.frames[i].source_bytes);
                assert(next.admission.frames[i].frame_kind == 3);
            }
        }
    } else {
        assert(false);
    }
    product_epm_receipts_carried_preserves(before, after);
    assert(product_score_wf(after));
}

pub proof fn product_epm_finish_accept_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
)
    requires product_score_wf(before), product_epm_finish_accept_step(before, after),
    ensures product_score_wf(after),
{
    if let Some(prior) = product_linked_top(before) {
        epm_source_accept::epm_source_finish_accept_preserves_wf(prior);
        assert(epm_source_accept::epm_program31_parent(prior.admission));
        let parent_index = before.bridge.admission.frames.len() - 2;
        assert(before.bridge.admission.frames[parent_index].frame_kind == 1);
        assert((parent_index as int) < before.epm_frames.len());
        assert(epm::indexed_image_wf(before.indexed_regions[parent_index as int]));
        assert(match before.epm_frames[parent_index as int] {
            None => true,
            Some(frame) => before.bridge.admission.frames[parent_index].frame_kind == 3
                && frame.snapshot == before.indexed_regions[parent_index]
                && epm::frame_layout_wf(frame),
        });
        match before.epm_frames[parent_index as int] {
            None => {},
            Some(_) => {
                assert(before.bridge.admission.frames[parent_index].frame_kind == 3);
                assert(false);
            },
        }
        assert(before.epm_frames[parent_index].is_none());
        assert(admission::admission_state_wf(after.bridge.admission));
        assert(after.bridge.admission.arena == before.bridge.admission.arena);
        assert(score_bridge::bridge_well_formed(after.bridge));
        assert(after.map.admission == after.bridge.admission);
        assert(after.map.transcript == after.bridge.transcript);
        assert(map::integrated_admission_wf(after.map));
        assert(after.epm_frames.len() == after.bridge.admission.frames.len());
        assert(after.indexed_regions.len() == after.bridge.admission.frames.len());
        assert forall|i: int| 0 <= i < after.epm_frames.len() implies {
            &&& epm::indexed_image_wf(after.indexed_regions[i])
            &&& after.indexed_regions[i].bytes
                == after.bridge.admission.frames[i].source_bytes
            &&& match after.epm_frames[i] {
                None => true,
                Some(frame) => {
                    &&& after.bridge.admission.frames[i].frame_kind == 3
                    &&& frame.snapshot == after.indexed_regions[i]
                    &&& epm::frame_layout_wf(frame)
                },
            }
        } by {
            assert(after.indexed_regions[i] == before.indexed_regions[i]);
            assert(epm::indexed_image_wf(before.indexed_regions[i]));
            assert(after.epm_frames[i] == before.epm_frames[i]);
            if i < after.epm_frames.len() - 1 {
                assert(after.bridge.admission.frames[i]
                    == before.bridge.admission.frames[i]);
            } else {
                assert(i == parent_index);
                assert(after.epm_frames[i].is_none());
                assert(after.bridge.admission.frames[i].source_bytes
                    == before.bridge.admission.frames[i].source_bytes);
            }
        }
        product_epm_finish_receipt_preserves(before, after);
        product_frame_source_anchors_pop(before, after);
        product_score_wf_intro(after);
    } else {
        assert(false);
    }
}

pub proof fn product_source_step_preserves_exact(
    before: ProductScoreState, after: ProductScoreState,
    action: ProductSourceAction,
)
    requires product_source_step(before, after, action),
    ensures product_score_wf(after),
{
    match action {
        ProductSourceAction::Map { action: map_action } => {
            product_map_step_preserves_exact(before, after, map_action);
        },
        ProductSourceAction::RootScore => {
            product_root_score_step_preserves_exact(before, after);
        },
        ProductSourceAction::RootEPathMapScore => {
            product_epm_source_root_score_selected_exact(before);
        },
        ProductSourceAction::ChildEPathMapScore => {
            product_epm_source_child_score_selected_exact(before);
        },
        ProductSourceAction::EpmHandoff { backing, plan, validated } => {
            product_epm_handoff_preserves_exact(before, after, backing, plan, validated);
        },
        ProductSourceAction::EpmContinue { event } => {
            product_epm_continue_preserves_exact(before, after, event);
        },
        ProductSourceAction::EpmPush { event, action, child } => {
            product_epm_push_preserves_exact(before, after, event, action, child);
        },
        ProductSourceAction::EpmScore => {
            product_epm_score_preserves_exact(before, after);
        },
        ProductSourceAction::EpmChildReturn {
            summary, admission_summary, nested_after,
        } => {
            product_epm_child_return_preserves_exact(
                before, after, summary, admission_summary, nested_after,
            );
        },
        ProductSourceAction::EpmComplete { action } => {
            product_epm_complete_preserves_exact(before, after, action);
        },
        ProductSourceAction::EpmFinishAccept => {
            product_epm_finish_accept_preserves_exact(before, after);
        },
    }
}

pub open spec fn product_root_trace(
    root: admission::AdmissionFrameState,
    backing: epm::IndexedBackingImage,
    states: Seq<ProductScoreState>,
    actions: Seq<ProductSourceAction>,
) -> bool {
    &&& states.len() == actions.len() + 1
    &&& product_root_initial(root, backing) == Some(states[0])
    &&& forall|i: int| 0 <= i < actions.len()
        ==> product_source_step(states[i], states[i + 1], actions[i])
}

pub proof fn product_root_trace_prefix_exact(
    root: admission::AdmissionFrameState,
    backing: epm::IndexedBackingImage,
    states: Seq<ProductScoreState>,
    actions: Seq<ProductSourceAction>,
    index: int,
)
    requires product_root_trace(root, backing, states, actions),
        0 <= index < states.len(),
    ensures product_score_wf(states[index]),
    decreases index,
{
    if index == 0 {
        product_root_initial_exact(root, backing);
    } else {
        product_root_trace_prefix_exact(root, backing, states, actions, index - 1);
        product_source_step_preserves_exact(
            states[index - 1], states[index], actions[index - 1],
        );
    }
}

pub struct LiveSourceProductState {
    pub ghost product: ProductScoreState,
    pub ghost runtime: runtime_source::RuntimeFrameImage,
    pub ghost arena: arena_source::RuntimeScoreArena,
    pub ghost wire: Seq<Option<wire_source::WireSourceState>>,
}

pub open spec fn live_source_product_wf(state: LiveSourceProductState) -> bool {
    &&& product_score_wf(state.product)
    &&& runtime_source::runtime_frame_image_wf(state.runtime)
    &&& state.runtime.score == state.product.bridge
    &&& arena_source::runtime_projects_score(
        state.arena, state.product.bridge.transcript,
    )
    &&& state.wire.len() == state.product.bridge.admission.frames.len()
    &&& forall|i: int| 0 <= i < state.wire.len() ==> {
        let frame = state.product.bridge.admission.frames[i];
        match #[trigger] state.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire_source::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    }
}

pub proof fn live_source_arena_entries_agree(state: LiveSourceProductState)
    requires live_source_product_wf(state),
    ensures
        state.arena.entries.len() == state.product.bridge.admission.arena.len(),
        forall|i: int| 0 <= i < state.arena.entries.len() ==>
            state.arena.entries[i].encoded_len
                == state.product.bridge.admission.arena[i].encoded_len
                && state.product.bridge.admission.arena[i].transcript_bytes
                    == state.product.bridge.transcript.entries[i].transcript,
{
    assert forall|i: int| 0 <= i < state.arena.entries.len() implies
        state.arena.entries[i].encoded_len
            == state.product.bridge.admission.arena[i].encoded_len
            && state.product.bridge.admission.arena[i].transcript_bytes
                == state.product.bridge.transcript.entries[i].transcript by {
        assert(state.arena.entries[i].encoded_len
            == state.product.bridge.transcript.entries[i].encoded_len);
        assert(state.product.bridge.admission.arena[i].encoded_len
            == state.product.bridge.transcript.entries[i].encoded_len);
    }
}

pub open spec fn live_source_product_continue_step(
    before: LiveSourceProductState, after: LiveSourceProductState,
) -> bool {
    if !live_source_product_wf(before) || before.wire.len() == 0 {
        false
    } else {
        match before.wire.last() {
            None => false,
            Some(reader) => match wire_source::wire_source_next_field(reader) {
                wire_source::WireSourceTransition::Field {
                    state: next_reader, field,
                } => {
                    &&& product_map_step(
                        before.product, after.product,
                        map::IntegratedAdmissionAction::Message {
                            action: admission::ConcreteFrameActionModel::Continue { field },
                        },
                    )
                    &&& runtime_source::runtime_continue(before.runtime, field)
                        == Some(after.runtime)
                    &&& after.arena == before.arena
                    &&& after.wire == before.wire.update(
                        before.wire.len() - 1, Some(next_reader),
                    )
                },
                _ => false,
            },
        }
    }
}

pub open spec fn live_source_product_continue_indexed_step(
    before: LiveSourceProductState, after: LiveSourceProductState,
) -> bool {
    if !live_source_product_wf(before) || before.wire.len() == 0 {
        false
    } else {
        let parent_source = product_admission_source_image(
            before.product.indexed_regions.last(),
        );
        match before.wire.last() {
            None => false,
            Some(reader) => match wire_source::wire_source_next_field(reader) {
                wire_source::WireSourceTransition::Field {
                    state: next_reader, field,
                } => {
                    &&& product_map_step(
                        before.product, after.product,
                        map::IntegratedAdmissionAction::Message {
                            action: admission::ConcreteFrameActionModel::ContinueIndexed {
                                field, parent_source,
                            },
                        },
                    )
                    &&& runtime_source::runtime_continue_indexed(
                        before.runtime, field, parent_source,
                    ) == Some(after.runtime)
                    &&& after.arena == before.arena
                    &&& after.wire == before.wire.update(
                        before.wire.len() - 1, Some(next_reader),
                    )
                },
                _ => false,
            },
        }
    }
}

pub proof fn live_source_product_continue_indexed_preserves_wf(
    before: LiveSourceProductState, after: LiveSourceProductState,
)
    requires live_source_product_continue_indexed_step(before, after),
    ensures live_source_product_wf(after),
{
    let parent_source = product_admission_source_image(
        before.product.indexed_regions.last(),
    );
    if let Some(reader) = before.wire.last() {
        if let wire_source::WireSourceTransition::Field {
            state: next_reader, field,
        } = wire_source::wire_source_next_field(reader) {
            wire_source::wire_source_next_field_refines(reader);
            product_map_step_preserves_exact(
                before.product, after.product,
                map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::ContinueIndexed {
                        field, parent_source,
                    },
                },
            );
            runtime_source::runtime_continue_indexed_retains_stack_and_score(
                before.runtime, field, parent_source,
            );
        }
    }
}

pub proof fn live_source_product_continue_field_is_read(
    before: LiveSourceProductState, after: LiveSourceProductState,
)
    requires live_source_product_continue_step(before, after),
    ensures exists|reader: wire_source::WireSourceState,
        next_reader: wire_source::WireSourceState,
        field: admission::IndexedCanonicalFieldModel| {
        &&& before.wire.last() == Some(reader)
        &&& wire_source::wire_source_next_field(reader)
            == wire_source::WireSourceTransition::Field {
                state: next_reader, field,
            }
        &&& product_map_step(
            before.product, after.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Continue { field },
            },
        )
        &&& runtime_source::runtime_continue(before.runtime, field)
            == Some(after.runtime)
        &&& after.wire == before.wire.update(
            before.wire.len() - 1, Some(next_reader),
        )
    },
{}

pub proof fn live_source_product_continue_preserves_wf(
    before: LiveSourceProductState, after: LiveSourceProductState,
)
    requires live_source_product_continue_step(before, after),
    ensures live_source_product_wf(after),
{
    if let Some(reader) = before.wire.last() {
        if let wire_source::WireSourceTransition::Field {
            state: next_reader, field,
        } = wire_source::wire_source_next_field(reader) {
            wire_source::wire_source_next_field_refines(reader);
            product_map_step_preserves_exact(
                before.product, after.product,
                map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::Continue { field },
                },
            );
            runtime_source::runtime_continue_retains_stack_and_score(before.runtime, field);
        }
    }
}

pub open spec fn live_source_product_map_key_step(
    before: LiveSourceProductState, after: LiveSourceProductState,
) -> bool {
    if !live_source_product_wf(before) || before.wire.len() == 0
        || before.product.bridge.admission.frames.last().frame_kind != 2
    {
        false
    } else {
        match before.wire.last() {
            None => false,
            Some(reader) => match wire_source::wire_source_next_field(reader) {
                wire_source::WireSourceTransition::Field {
                    state: next_reader, field,
                } => {
                    &&& product_map_step(
                        before.product, after.product,
                        map::IntegratedAdmissionAction::MapKey { field },
                    )
                    &&& after.runtime == runtime_source::RuntimeFrameImage {
                        score: after.product.bridge,
                        ..before.runtime
                    }
                    &&& after.arena == before.arena
                    &&& after.wire == before.wire.update(
                        before.wire.len() - 1, Some(next_reader),
                    )
                },
                _ => false,
            },
        }
    }
}

pub proof fn live_source_product_map_key_preserves_wf(
    before: LiveSourceProductState, after: LiveSourceProductState,
)
    requires live_source_product_map_key_step(before, after),
    ensures live_source_product_wf(after),
{
    let reader = before.wire.last().unwrap();
    if let wire_source::WireSourceTransition::Field {
        state: next_reader, field,
    } = wire_source::wire_source_next_field(reader) {
        wire_source::wire_source_next_field_refines(reader);
        product_map_step_preserves_exact(
            before.product, after.product,
            map::IntegratedAdmissionAction::MapKey { field },
        );
        assert(after.product.bridge.transcript
            == before.product.bridge.transcript);
        assert(runtime_source::runtime_frame_image_wf(after.runtime));
        assert forall|i: int| 0 <= i < after.wire.len() implies {
            let frame = after.product.bridge.admission.frames[i];
            match #[trigger] after.wire[i] {
                None => frame.frame_kind == 3 || frame.frame_kind == 4,
                Some(next) => {
                    &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                    &&& wire_source::wire_cursor_shape(next.cursor)
                    &&& next.cursor.region_start == frame.region_start
                    &&& next.cursor.region_end == frame.region_end
                    &&& next.cursor.next == frame.cursor
                    &&& next.cursor.source_bytes == frame.source_bytes
                    &&& next.terminal_error.is_none()
                },
            }
        } by {
            if i < after.wire.len() - 1 {
                assert(after.wire[i] == before.wire[i]);
                assert(after.product.bridge.admission.frames[i]
                    == before.product.bridge.admission.frames[i]);
            } else {
                assert(after.wire[i] == Some(next_reader));
                assert(next_reader.cursor.next == field.encoded_end);
            }
        }
    }
}

pub struct LiveRuntimeProductState {
    pub ghost source: LiveSourceProductState,
    pub ghost cursors: Seq<Option<wire_source::WireRuntimeCursorImage>>,
}

#[cfg(rpb3_live_push)]
pub struct CheckedMessagePushPrestateSnapshot {
    pub cursor: usize,
    pub last_slot: Option<usize>,
    pub slot: usize,
    pub seen_before: bool,
}

#[cfg(rpb3_live_push)]
pub struct CheckedMessageFieldBeforeImage {
    pub cursor: usize,
    pub last_slot: Option<usize>,
    pub slot: usize,
    pub seen: bool,
}

#[cfg(rpb3_live_push)]
pub open spec fn checked_message_push_snapshot_from_field_before(
    before: CheckedMessageFieldBeforeImage,
) -> CheckedMessagePushPrestateSnapshot {
    CheckedMessagePushPrestateSnapshot {
        cursor: before.cursor,
        last_slot: before.last_slot,
        slot: before.slot,
        seen_before: before.seen,
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn checked_message_push_receipt_from_field_before(
    template: rpb3_message_driver_source_integration::GeneratedMessagePushReceipt,
    before: CheckedMessageFieldBeforeImage,
) -> rpb3_message_driver_source_integration::GeneratedMessagePushReceipt {
    rpb3_message_driver_source_integration::GeneratedMessagePushReceipt {
        cursor_before: before.cursor,
        last_slot_before: before.last_slot,
        seen_before: before.seen,
        ..template
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn checked_message_push_snapshot_matches_live(
    snapshot: CheckedMessagePushPrestateSnapshot,
    live: LiveRuntimeProductState,
) -> bool {
    if live.source.product.bridge.admission.frames.len() == 0 {
        false
    } else {
        let frame = live.source.product.bridge.admission.frames.last();
        &&& snapshot.cursor == frame.cursor
        &&& snapshot.last_slot == frame.last_slot
        &&& snapshot.slot < frame.slot_count
        &&& snapshot.seen_before == frame.seen.contains(snapshot.slot as int)
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn checked_message_push_source_from_live(
    live: LiveRuntimeProductState,
) -> Option<rpb3_message_driver_source_integration::GeneratedMessageFieldSource> {
    if live.source.product.bridge.admission.frames.len() == 0
        || live.source.product.indexed_regions.len() == 0
    {
        None
    } else {
        let frame = live.source.product.bridge.admission.frames.last();
        Some(rpb3_message_driver_source_integration::GeneratedMessageFieldSource {
            program_type_id: frame.program_type_id,
            family: frame.family,
            slot_count: frame.slot_count,
            body_id: frame.body_id,
            body_len: frame.region_end,
            indexed: product_admission_source_image(
                live.source.product.indexed_regions.last(),
            ),
        })
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn checked_message_push_fold_from_live(
    live: LiveRuntimeProductState,
) -> Option<rpb3_generic_message_eof_source_refinement::GenericMessageSourceFold> {
    if live.source.product.bridge.admission.frames.len() == 0 {
        None
    } else {
        let frame = live.source.product.bridge.admission.frames.last();
        Some(rpb3_generic_message_eof_source_refinement::GenericMessageSourceFold {
            cursor: frame.cursor,
            last_slot: frame.last_slot,
            seen: frame.seen,
            scalar_slots: frame.scalar_slots,
            scalar_occurrences: frame.scalar_occurrences,
            returns: frame.returns,
            pending: None,
        })
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn checked_message_push_fold_core_matches_live(
    source: rpb3_message_driver_source_integration::GeneratedMessageFieldSource,
    folded: rpb3_generic_message_eof_source_refinement::GenericMessageSourceFold,
    live: LiveRuntimeProductState,
) -> bool {
    if !live_runtime_product_wf(live)
        || live.source.product.bridge.admission.frames.len() == 0
        || live.source.product.indexed_regions.len() == 0
    {
        false
    } else {
        let frame = live.source.product.bridge.admission.frames.last();
        &&& frame.frame_kind == 1
        &&& frame.region_start == 0
        &&& frame.region_end as int == frame.source_bytes.len()
        &&& checked_message_push_source_from_live(live) == Some(source)
        &&& checked_message_push_fold_from_live(live) == Some(folded)
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn checked_message_push_trace_core_matches_live(
    source: rpb3_message_driver_source_integration::GeneratedMessageFieldSource,
    operations: Seq<rpb3_message_driver_source_integration::GeneratedMessageSourceOperation>,
    live: LiveRuntimeProductState,
) -> bool {
    match rpb3_message_driver_source_integration::generated_message_replay_from_source_operations(
        source, operations,
    ) {
        Some(replay) => checked_message_push_fold_core_matches_live(
            source, replay.fields, live,
        ),
        None => false,
    }
}

#[cfg(rpb3_live_push)]
pub proof fn checked_message_replay_step_preserves_full_core(
    source: rpb3_message_driver_source_integration::GeneratedMessageFieldSource,
    replay: rpb3_message_driver_source_integration::GeneratedMessageEventReplay,
    operation: rpb3_message_driver_source_integration::GeneratedMessageSourceOperation,
)
    requires rpb3_message_driver_source_integration::generated_message_replay_source_step(
        source, replay, operation,
    ).is_some(),
    ensures {
        let next = rpb3_message_driver_source_integration::generated_message_replay_source_step(
            source, replay, operation,
        ).unwrap();
        &&& rpb3_message_driver_source_integration::generated_message_event_replay_wf(
            source, next,
        )
        &&& next.events.len() == replay.events.len() + 1
        &&& match operation {
            rpb3_message_driver_source_integration::GeneratedMessageSourceOperation::Field {
                action: rpb3_message_driver_source_integration::GeneratedMessageFieldAction::Scalar {
                    receipt,
                },
            } => {
                &&& next.fields.cursor == receipt.field.encoded_end
                &&& next.fields.last_slot == Some(receipt.slot)
                &&& next.fields.seen == replay.fields.seen.insert(receipt.slot as int)
                &&& next.fields.scalar_slots
                    == replay.fields.scalar_slots.insert(receipt.slot as int)
                &&& next.fields.scalar_occurrences.len()
                    == replay.fields.scalar_occurrences.len() + 1
                &&& next.fields.returns == replay.fields.returns
                &&& next.fields.pending.is_none()
            },
            rpb3_message_driver_source_integration::GeneratedMessageSourceOperation::Field {
                action: rpb3_message_driver_source_integration::GeneratedMessageFieldAction::Push {
                    receipt,
                },
            } => {
                &&& next.fields.cursor == receipt.field.encoded_end
                &&& next.fields.last_slot == Some(receipt.slot)
                &&& next.fields.seen == replay.fields.seen.insert(receipt.slot as int)
                &&& next.fields.scalar_slots == replay.fields.scalar_slots
                &&& next.fields.scalar_occurrences == replay.fields.scalar_occurrences
                &&& next.fields.returns == replay.fields.returns
                &&& next.fields.pending
                    == Some((receipt.slot, receipt.field.number, receipt.body_id,
                        receipt.field.payload_bytes))
            },
            rpb3_message_driver_source_integration::GeneratedMessageSourceOperation::Accepted {
                returned,
            } => {
                &&& next.fields.cursor == replay.fields.cursor
                &&& next.fields.last_slot == replay.fields.last_slot
                &&& next.fields.seen == replay.fields.seen
                &&& next.fields.scalar_slots == replay.fields.scalar_slots
                &&& next.fields.scalar_occurrences == replay.fields.scalar_occurrences
                &&& next.fields.returns == replay.fields.returns.push(returned)
                &&& next.fields.pending.is_none()
            },
        }
    },
{}

#[cfg(rpb3_live_push)]
pub proof fn checked_message_push_snapshot_and_core_establish_live_fold(
    source: rpb3_message_driver_source_integration::GeneratedMessageFieldSource,
    operations: Seq<rpb3_message_driver_source_integration::GeneratedMessageSourceOperation>,
    live: LiveRuntimeProductState,
    before: CheckedMessageFieldBeforeImage,
    template: rpb3_message_driver_source_integration::GeneratedMessagePushReceipt,
)
    requires
        rpb3_message_driver_source_integration::generated_message_replay_from_source_operations(
            source, operations,
        ).is_some(),
        checked_message_push_snapshot_matches_live(
            checked_message_push_snapshot_from_field_before(before), live,
        ),
        checked_message_push_trace_core_matches_live(source, operations, live),
    ensures {
        let replay = rpb3_message_driver_source_integration::generated_message_replay_from_source_operations(
            source, operations,
        ).unwrap();
        let folded = replay.fields;
        let receipt = checked_message_push_receipt_from_field_before(template, before);
        &&& rpb3_message_driver_source_integration::generated_message_live_frame_matches_fold(
            source, folded, live,
        )
        &&& receipt.cursor_before == before.cursor
        &&& receipt.last_slot_before == before.last_slot
        &&& receipt.seen_before == before.seen
        &&& receipt.cursor_before == folded.cursor
        &&& receipt.last_slot_before == folded.last_slot
        &&& receipt.seen_before == folded.seen.contains(before.slot as int)
    },
{
    let replay = rpb3_message_driver_source_integration::generated_message_replay_from_source_operations(
        source, operations,
    ).unwrap();
    let folded = replay.fields;
    rpb3_message_driver_source_integration::generated_message_replay_from_source_operations_exact(
        source, operations,
    );
    let frame = live.source.product.bridge.admission.frames.last();
    assert(checked_message_push_source_from_live(live) == Some(source));
    assert(checked_message_push_fold_from_live(live) == Some(folded));
    assert(product_score_wf(live.source.product));
    assert(live.source.product.indexed_regions.last().bytes == frame.source_bytes);
    assert(source.indexed.bytes == frame.source_bytes);
    assert(folded.cursor == frame.cursor);
    assert(folded.last_slot == frame.last_slot);
    assert(folded.seen == frame.seen);
    assert(before.cursor == frame.cursor);
    assert(before.last_slot == frame.last_slot);
    assert(before.seen == frame.seen.contains(before.slot as int));
}

pub open spec fn product_runtime_kind_from_score(
    kind: rpb3_score_transcript::ScoreNode,
) -> arena_source::RuntimeScoreKind {
    match kind {
        rpb3_score_transcript::ScoreNode::Integer(value) =>
            arena_source::RuntimeScoreKind::Integer(value),
        rpb3_score_transcript::ScoreNode::StringAtom(bytes) =>
            arena_source::RuntimeScoreKind::StringOwned(bytes),
        rpb3_score_transcript::ScoreNode::BytesAtom(bytes) =>
            arena_source::RuntimeScoreKind::BytesOwned(bytes),
        rpb3_score_transcript::ScoreNode::Node(children) =>
            arena_source::RuntimeScoreKind::Node(children),
    }
}

pub open spec fn product_runtime_arena_from_score(
    transcript: rpb3_score_transcript::ScoreArena,
) -> arena_source::RuntimeScoreArena {
    arena_source::RuntimeScoreArena {
        entries: Seq::new(transcript.entries.len(), |i: int|
            arena_source::RuntimeScoreEntry {
                kind: product_runtime_kind_from_score(transcript.entries[i].kind),
                encoded_len: transcript.entries[i].encoded_len,
            },
        ),
    }
}

pub proof fn product_runtime_arena_from_score_projects(
    transcript: rpb3_score_transcript::ScoreArena,
)
    requires rpb3_score_transcript::arena_well_formed(transcript),
    ensures arena_source::runtime_projects_score(
        product_runtime_arena_from_score(transcript), transcript,
    ),
{
    let runtime = product_runtime_arena_from_score(transcript);
    assert forall|i: int| 0 <= i < runtime.entries.len() implies {
        &&& arena_source::runtime_kind_image(runtime.entries[i].kind)
            == transcript.entries[i].kind
        &&& runtime.entries[i].encoded_len == transcript.entries[i].encoded_len
    } by {
        match transcript.entries[i].kind {
            rpb3_score_transcript::ScoreNode::Integer(_) => {},
            rpb3_score_transcript::ScoreNode::StringAtom(_) => {},
            rpb3_score_transcript::ScoreNode::BytesAtom(_) => {},
            rpb3_score_transcript::ScoreNode::Node(_) => {},
        }
    }
}

pub open spec fn live_runtime_product_wf(state: LiveRuntimeProductState) -> bool {
    &&& live_source_product_wf(state.source)
    &&& state.cursors.len() == state.source.wire.len()
    &&& forall|i: int| 0 <= i < state.cursors.len() ==> match #[trigger] state.cursors[i] {
        None => true,
        Some(cursor) => product_wire_indexed_source_agrees(
            cursor, state.source.product.indexed_regions[i],
        ),
    }
    &&& forall|i: int| 0 <= i < state.cursors.len() ==> {
        match (#[trigger] state.cursors[i], state.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    }
}

pub proof fn live_runtime_wire_sources_steady(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        live_runtime_product_wf(before),
        before.cursors.len() > 0,
        after.cursors.len() == before.cursors.len(),
        after.source.product.indexed_regions
            == before.source.product.indexed_regions,
        forall|i: int| 0 <= i < before.cursors.len() - 1
            ==> after.cursors[i] == before.cursors[i],
        before.cursors.last().is_some(),
        after.cursors.last().is_some(),
        after.cursors.last().unwrap().message
            == before.cursors.last().unwrap().message,
    ensures forall|i: int| 0 <= i < after.cursors.len()
        ==> match #[trigger] after.cursors[i] {
            None => true,
            Some(cursor) => product_wire_indexed_source_agrees(
                cursor, after.source.product.indexed_regions[i],
            ),
        },
{
    assert forall|i: int| 0 <= i < after.cursors.len()
        implies match #[trigger] after.cursors[i] {
            None => true,
            Some(cursor) => product_wire_indexed_source_agrees(
                cursor, after.source.product.indexed_regions[i],
            ),
        } by {
        if i < before.cursors.len() - 1 {
            assert(after.cursors[i] == before.cursors[i]);
            assert(after.source.product.indexed_regions[i]
                == before.source.product.indexed_regions[i]);
        } else {
            let old = before.cursors.last().unwrap();
            let new = after.cursors.last().unwrap();
            product_wire_indexed_source_agrees_same_backing(
                old, new, before.source.product.indexed_regions.last(),
            );
        }
    }
}

pub proof fn live_runtime_wire_sources_pop(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        live_runtime_product_wf(before),
        after.cursors == before.cursors.drop_last(),
        after.source.product.indexed_regions
            == before.source.product.indexed_regions.drop_last(),
    ensures forall|i: int| 0 <= i < after.cursors.len()
        ==> match #[trigger] after.cursors[i] {
            None => true,
            Some(cursor) => product_wire_indexed_source_agrees(
                cursor, after.source.product.indexed_regions[i],
            ),
        },
{
    assert forall|i: int| 0 <= i < after.cursors.len()
        implies match #[trigger] after.cursors[i] {
            None => true,
            Some(cursor) => product_wire_indexed_source_agrees(
                cursor, after.source.product.indexed_regions[i],
            ),
        } by {
        assert(after.cursors[i] == before.cursors[i]);
        assert(after.source.product.indexed_regions[i]
            == before.source.product.indexed_regions[i]);
    }
}

pub open spec fn runtime_wire_after_field(
    before: wire_source::WireRuntimeCursorImage,
    after: wire_source::WireConcreteCursorImage,
) -> wire_source::WireRuntimeCursorImage {
    wire_source::WireRuntimeCursorImage {
        message: before.message,
        reader_view: before.reader_view,
        reader_at: after.reader_at,
        cursor_yielded: after.cursor_yielded,
        last_field: after.last_field,
        enforce_numeric_order: after.enforce_numeric_order,
        fields_read: after.fields_read,
        terminal_error: after.terminal_error,
    }
}

pub proof fn product_runtime_epm_source_score_selected_matches_product(
    product: ProductScoreState,
    runtime: runtime_source::RuntimeFrameImage,
)
    requires
        product_score_wf(product),
        runtime_source::runtime_frame_image_wf(runtime),
        runtime.score == product.bridge,
        product.bridge.admission.frames.len() > 0,
        product.accepted_epm_sources.last().is_some(),
        product.bridge.admission.frames.last().cursor
            == product.bridge.admission.frames.last().region_end,
        product_epm_source_bridge_finish_selected(product).is_some(),
    ensures {
        let bytes = product.accepted_epm_sources.last().unwrap()
            .epm.snapshot.bytes;
        let next = product_epm_source_bridge_finish_selected(product).unwrap().0;
        runtime_source::runtime_epm_source_score_finish_selected(
            runtime, bytes,
        ) == Some(next)
    },
{
    product_epm_source_receipt_binds_top_eof(product);
}

pub proof fn live_runtime_epm_source_score_selected_matches_product(
    before: LiveRuntimeProductState,
)
    requires
        live_runtime_product_wf(before),
        before.source.product.bridge.admission.frames.len() > 0,
        before.source.product.accepted_epm_sources.last().is_some(),
        before.source.product.bridge.admission.frames.last().cursor
            == before.source.product.bridge.admission.frames.last().region_end,
        product_epm_source_bridge_finish_selected(before.source.product).is_some(),
    ensures {
        let bytes = before.source.product.accepted_epm_sources.last().unwrap()
            .epm.snapshot.bytes;
        let next = product_epm_source_bridge_finish_selected(
            before.source.product,
        ).unwrap().0;
        runtime_source::runtime_epm_source_score_finish_selected(
            before.source.runtime, bytes,
        ) == Some(next)
    },
{
    product_runtime_epm_source_score_selected_matches_product(
        before.source.product, before.source.runtime,
    );
}

pub open spec fn live_runtime_epm_source_eof_selected(
    before: LiveRuntimeProductState,
) -> Option<runtime_source::RuntimeMessageOutcome> {
    if !live_runtime_product_wf(before)
        || before.source.product.bridge.admission.frames.len() == 0
        || before.source.product.accepted_epm_sources.last().is_none()
        || before.source.product.bridge.admission.frames.last().cursor
            != before.source.product.bridge.admission.frames.last().region_end
        || before.cursors.last().is_none()
        || wire_source::wire_runtime_next_field(before.cursors.last().unwrap())
            != Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    before.cursors.last().unwrap(),
                ).unwrap(),
            })
    {
        None
    } else {
        let bytes = before.source.product.accepted_epm_sources.last().unwrap()
            .epm.snapshot.bytes;
        runtime_source::runtime_message_driver_step(
            before.source.runtime,
            runtime_source::RuntimeMessageEvent::EofEPathMap {
                snapshot_bytes: bytes,
            },
        )
    }
}

pub proof fn live_runtime_epm_source_eof_dispatch_exact(
    before: LiveRuntimeProductState,
)
    requires
        live_runtime_product_wf(before),
        before.source.product.bridge.admission.frames.len() > 0,
        before.source.product.accepted_epm_sources.last().is_some(),
        before.source.product.bridge.admission.frames.last().cursor
            == before.source.product.bridge.admission.frames.last().region_end,
        before.cursors.last().is_some(),
        wire_source::wire_runtime_next_field(before.cursors.last().unwrap())
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    before.cursors.last().unwrap(),
                ).unwrap(),
            }),
        product_epm_source_bridge_finish_selected(before.source.product).is_some(),
    ensures {
        let bytes = before.source.product.accepted_epm_sources.last().unwrap()
            .epm.snapshot.bytes;
        let scored = product_epm_source_bridge_finish_selected(
            before.source.product,
        ).unwrap().0;
        let result = live_runtime_epm_source_eof_selected(before);
        &&& runtime_source::runtime_epm_source_score_finish_selected(
            before.source.runtime, bytes,
        ) == Some(scored)
        &&& result == runtime_source::runtime_message_driver_step(
            before.source.runtime,
            runtime_source::RuntimeMessageEvent::EofEPathMap {
                snapshot_bytes: bytes,
            },
        )
        &&& match result {
            Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }) => {
                &&& before.source.product.bridge.admission.frames.len() == 1
                &&& returned.pre_pop.score == scored
                &&& returned.remaining_frames == 0
            },
            Some(runtime_source::RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& before.source.product.bridge.admission.frames.len() > 1
                &&& runtime_source::runtime_pre_pop_child_accept(
                    runtime_source::RuntimeFrameImage {
                        score: scored,
                        ..before.source.runtime
                    },
                ) == Some(image)
            },
            _ => true,
        }
    },
{
    live_runtime_epm_source_score_selected_matches_product(before);
}

pub open spec fn live_runtime_product_epm_end_source_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    observed: rpb3_epathmap_terminal_source_observation::EpmTerminalSourceObservation,
) -> bool {
    if !live_runtime_product_wf(before)
        || product_linked_top(before.source.product).is_none()
        || rpb3_epathmap_terminal_source_observation::epm_terminal_complete_from_source(
            product_linked_top(before.source.product).unwrap().epm, observed,
        ).is_none()
    {
        false
    } else {
        let complete =
            rpb3_epathmap_terminal_source_observation::epm_terminal_complete_from_source(
                product_linked_top(before.source.product).unwrap().epm, observed,
            ).unwrap();
        &&& product_epm_complete_step(
            before.source.product, after.source.product, complete.action,
        )
        &&& after.source.product.epm_frames.last() == Some(complete.after)
        &&& after.source.runtime == before.source.runtime
        &&& after.source.arena == before.source.arena
        &&& after.source.wire == before.source.wire
        &&& after.cursors == before.cursors
    }
}

pub proof fn live_runtime_product_epm_end_source_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    observed: rpb3_epathmap_terminal_source_observation::EpmTerminalSourceObservation,
)
    requires live_runtime_product_epm_end_source_step(before, after, observed),
    ensures
        live_runtime_product_wf(after),
        after.source.product.bridge.admission == before.source.product.bridge.admission,
        after.source.product.bridge.transcript == before.source.product.bridge.transcript,
        after.source.product.indexed_regions == before.source.product.indexed_regions,
        after.source.product.epm_frames.last().is_some(),
        after.source.product.epm_frames.last().unwrap().cursor.done,
{
    let prior = product_linked_top(before.source.product).unwrap();
    let complete =
        rpb3_epathmap_terminal_source_observation::epm_terminal_complete_from_source(
            prior.epm, observed,
        ).unwrap();
    rpb3_epathmap_terminal_source_observation::epm_terminal_source_success_is_exact(
        prior.epm, observed,
    );
    product_epm_complete_preserves_exact(
        before.source.product, after.source.product, complete.action,
    );
    assert(after.source.product.bridge.admission
        == before.source.product.bridge.admission);
    assert(after.source.product.bridge.transcript
        == before.source.product.bridge.transcript);
    assert(after.source.product.indexed_regions
        == before.source.product.indexed_regions);
    assert(after.source.product.epm_frames.last() == Some(complete.after));
    assert(complete.after.cursor.done);
    assert(live_source_product_wf(after.source));
    assert(live_runtime_product_wf(after));
}

pub open spec fn live_source_product_epm_finish_accept_step(
    before: LiveSourceProductState, after: LiveSourceProductState,
) -> bool {
    &&& live_source_product_wf(before)
    &&& product_epm_finish_accept_step(before.product, after.product)
    &&& after.runtime == runtime_source::runtime_after_admission_step(
        before.runtime, after.product.bridge.admission,
    )
    &&& after.arena == before.arena
    &&& after.wire == before.wire.drop_last()
}

pub open spec fn live_runtime_product_epm_finish_accept_step(
    before: LiveRuntimeProductState, after: LiveRuntimeProductState,
) -> bool {
    &&& live_runtime_product_wf(before)
    &&& before.cursors.len() > 1
    &&& before.cursors.last().is_none()
    &&& live_source_product_epm_finish_accept_step(before.source, after.source)
    &&& after.cursors == before.cursors.drop_last()
}

pub proof fn live_source_product_epm_finish_accept_preserves_wf(
    before: LiveSourceProductState, after: LiveSourceProductState,
)
    requires live_source_product_epm_finish_accept_step(before, after),
    ensures
        live_source_product_wf(after),
        after.product.bridge.admission.frames.len() + 1
            == before.product.bridge.admission.frames.len(),
{
    product_epm_finish_accept_preserves_exact(before.product, after.product);
    let source = product_linked_top(before.product).unwrap();
    let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
    let finished = epm_source_accept::epm_end_linked(source);
    epm_source_accept::epm_source_accept_carries_end_summary(source, finished, accepted);
    assert(after.runtime.score == after.product.bridge);
    assert(after.runtime.score.admission.frames.len() + 1
        == before.runtime.score.admission.frames.len());
    assert(after.runtime.score.admission.frames.len() > 0);
    assert(after.runtime.capacity == before.runtime.capacity);
    assert(after.runtime.frame_bytes == before.runtime.frame_bytes);
    assert(after.runtime.max_frames == before.runtime.max_frames);
    assert(after.runtime.max_storage_bytes == before.runtime.max_storage_bytes);
    assert(after.runtime.max_capacity_bytes == before.runtime.max_capacity_bytes);
    assert(after.runtime.score.admission.frames.len()
        <= after.runtime.capacity as int);
    assert(after.runtime.max_frames as int
        >= after.runtime.score.admission.frames.len());
    let old_len = before.runtime.score.admission.frames.len();
    let new_len = after.runtime.score.admission.frames.len();
    let frame_bytes = after.runtime.frame_bytes as int;
    let max_storage_bytes = after.runtime.max_storage_bytes as int;
    assert(max_storage_bytes >= old_len * frame_bytes);
    assert(new_len <= old_len);
    assert(frame_bytes > 0);
    assert(max_storage_bytes >= new_len * frame_bytes)
        by (nonlinear_arith)
        requires
            max_storage_bytes >= old_len * frame_bytes,
            new_len <= old_len,
            frame_bytes > 0;
    assert(runtime_source::runtime_frame_image_wf(after.runtime));
    assert(arena_source::runtime_projects_score(
        after.arena, after.product.bridge.transcript,
    ));
    assert(after.wire.len() == after.product.bridge.admission.frames.len());
    assert forall|i: int| 0 <= i < after.wire.len() implies {
        let frame = after.product.bridge.admission.frames[i];
        match #[trigger] after.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire_source::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    } by {
        assert(after.wire[i] == before.wire[i]);
        let old = before.product.bridge.admission.frames[i];
        let new = after.product.bridge.admission.frames[i];
        assert(new.frame_kind == old.frame_kind);
        assert(new.region_start == old.region_start);
        assert(new.region_end == old.region_end);
        assert(new.cursor == old.cursor);
        assert(new.source_bytes == old.source_bytes);
    }
}

pub proof fn live_runtime_product_epm_finish_accept_preserves_wf(
    before: LiveRuntimeProductState, after: LiveRuntimeProductState,
)
    requires live_runtime_product_epm_finish_accept_step(before, after),
    ensures
        live_runtime_product_wf(after),
        after.source.product.bridge.admission.frames.len() + 1
            == before.source.product.bridge.admission.frames.len(),
{
    live_source_product_epm_finish_accept_preserves_wf(before.source, after.source);
    live_runtime_wire_sources_pop(before, after);
    assert(after.cursors.len() == after.source.wire.len());
    assert forall|i: int| 0 <= i < after.cursors.len() implies {
        match (#[trigger] after.cursors[i], after.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(after.cursors[i] == before.cursors[i]);
        assert(after.source.wire[i] == before.source.wire[i]);
    }
}

pub open spec fn live_epm_modeled_empty_runtime_cursor()
    -> wire_source::WireRuntimeCursorImage {
    let backing = wire_source::WireRuntimeBackingImage::Borrowed {
        bytes: product_epm_modeled_empty_snapshot_bytes(),
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 8,
        end_override: None,
    };
    wire_source::WireRuntimeCursorImage {
        message: backing,
        reader_view: backing,
        reader_at: 8,
        cursor_yielded: 8,
        last_field: 32,
        enforce_numeric_order: false,
        fields_read: 1,
        terminal_error: None,
    }
}

pub open spec fn live_epm_modeled_empty_nested_before()
    -> LiveRuntimeProductState {
    let source = product_epm_modeled_empty_nested_source();
    let product = product_epm_nested_source_witness_before(source);
    let cursor = live_epm_modeled_empty_runtime_cursor();
    let reader = wire_source::wire_runtime_cursor_project(cursor).unwrap();
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product,
            runtime: runtime_source::RuntimeFrameImage {
                score: product.bridge,
                capacity: 3,
                frame_bytes: 1,
                max_frames: 3,
                max_storage_bytes: 3,
                max_capacity_bytes: 3,
            },
            arena: arena_source::RuntimeScoreArena { entries: Seq::empty() },
            wire: seq![Some(reader), Some(reader), None],
        },
        cursors: seq![Some(cursor), Some(cursor), None],
    }
}

pub open spec fn live_epm_modeled_empty_nested_after_accept()
    -> LiveRuntimeProductState {
    let before = live_epm_modeled_empty_nested_before();
    let source = product_epm_modeled_empty_nested_source();
    let product = product_epm_nested_source_witness_after(source);
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product,
            runtime: runtime_source::runtime_after_admission_step(
                before.source.runtime, product.bridge.admission,
            ),
            arena: before.source.arena,
            wire: before.source.wire.drop_last(),
        },
        cursors: before.cursors.drop_last(),
    }
}

pub proof fn live_epm_modeled_empty_nested_cursor_ready()
    ensures {
        let cursor = live_epm_modeled_empty_runtime_cursor();
        &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
        &&& wire_source::wire_runtime_cursor_project(cursor).is_some()
        &&& wire_source::wire_concrete_cursor_wf(
            wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
        )
        &&& wire_source::wire_runtime_next_field(cursor)
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
            })
    },
{
    let cursor = live_epm_modeled_empty_runtime_cursor();
    assert(wire_source::wire_runtime_derive_cursor(cursor).is_some());
    assert(wire_source::wire_runtime_cursor_project(cursor).is_some());
    assert(wire_source::wire_concrete_cursor_wf(
        wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
    ));
    wire_source::wire_runtime_end_is_exact(cursor);
}

pub proof fn live_epm_modeled_empty_nested_source_before_wf()
    ensures live_source_product_wf(
        live_epm_modeled_empty_nested_before().source,
    ),
{
    product_epm_modeled_empty_nested_source_lifts_accept();
    live_epm_modeled_empty_nested_cursor_ready();
    let before = live_epm_modeled_empty_nested_before();
    assert(arena_source::runtime_projects_score(
        before.source.arena, before.source.product.bridge.transcript,
    ));
    assert(runtime_source::runtime_frame_image_wf(before.source.runtime));
    assert(live_source_product_wf(before.source));
}

pub proof fn live_epm_modeled_empty_nested_before_wf()
    ensures live_runtime_product_wf(live_epm_modeled_empty_nested_before()),
{
    live_epm_modeled_empty_nested_source_before_wf();
    let before = live_epm_modeled_empty_nested_before();
    let cursor = live_epm_modeled_empty_runtime_cursor();
    assert(before.cursors.len() == before.source.wire.len());
    assert forall|i: int| 0 <= i < before.cursors.len()
        implies match #[trigger] before.cursors[i] {
            None => true,
            Some(cursor) => product_wire_indexed_source_agrees(
                cursor, before.source.product.indexed_regions[i],
            ),
        } by {
        assert(i == 0 || i == 1 || i == 2);
        if i < 2 {
            assert(before.cursors[i] == Some(cursor));
        }
    }
    assert forall|i: int| 0 <= i < before.cursors.len() implies {
        match (#[trigger] before.cursors[i], before.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(i == 0 || i == 1 || i == 2);
        if i < 2 {
            assert(before.cursors[i] == Some(cursor));
        }
    }
}

pub proof fn live_epm_modeled_empty_nested_accept_step_exact()
    ensures {
        let before = live_epm_modeled_empty_nested_before();
        let after = live_epm_modeled_empty_nested_after_accept();
        &&& live_runtime_product_epm_finish_accept_step(before, after)
        &&& after.source.product.accepted_epm_sources.last()
            == Some(product_epm_modeled_empty_nested_source())
    },
{
    product_epm_modeled_empty_nested_source_lifts_accept();
    live_epm_modeled_empty_nested_before_wf();
    let before = live_epm_modeled_empty_nested_before();
    let after = live_epm_modeled_empty_nested_after_accept();
    assert(live_runtime_product_epm_finish_accept_step(before, after));
}

pub proof fn live_epm_modeled_empty_nested_after_wf()
    ensures live_runtime_product_wf(live_epm_modeled_empty_nested_after_accept()),
{
    live_epm_modeled_empty_nested_accept_step_exact();
    let before = live_epm_modeled_empty_nested_before();
    let after = live_epm_modeled_empty_nested_after_accept();
    live_runtime_product_epm_finish_accept_preserves_wf(before, after);
}

pub proof fn live_epm_modeled_empty_nested_after_cursor_end()
    ensures {
        let after = live_epm_modeled_empty_nested_after_accept();
        &&& after.cursors.last().is_some()
        &&& wire_source::wire_runtime_next_field(after.cursors.last().unwrap())
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    after.cursors.last().unwrap(),
                ).unwrap(),
            })
    },
{
    live_epm_modeled_empty_nested_cursor_ready();
    let before = live_epm_modeled_empty_nested_before();
    let after = live_epm_modeled_empty_nested_after_accept();
    let cursor = live_epm_modeled_empty_runtime_cursor();
    assert(after.cursors == before.cursors.drop_last());
    assert(after.cursors.last() == Some(cursor));
}

pub proof fn live_epm_modeled_empty_nested_accept_witness()
    ensures {
        let before = live_epm_modeled_empty_nested_before();
        let after = live_epm_modeled_empty_nested_after_accept();
        &&& live_runtime_product_wf(before)
        &&& live_runtime_product_epm_finish_accept_step(before, after)
        &&& live_runtime_product_wf(after)
        &&& after.source.product.accepted_epm_sources.last()
            == Some(product_epm_modeled_empty_nested_source())
        &&& after.cursors.last().is_some()
        &&& wire_source::wire_runtime_next_field(after.cursors.last().unwrap())
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    after.cursors.last().unwrap(),
                ).unwrap(),
            })
    },
{
    live_epm_modeled_empty_nested_before_wf();
    live_epm_modeled_empty_nested_accept_step_exact();
    live_epm_modeled_empty_nested_after_wf();
    live_epm_modeled_empty_nested_after_cursor_end();
}

pub proof fn product_epm_modeled_empty_nested_scores()
    ensures product_epm_source_child_score_selected(
        product_epm_nested_source_witness_after(
            product_epm_modeled_empty_nested_source(),
        ),
    ).is_some(),
{
    product_epm_modeled_empty_nested_source_lifts_accept();
    let state = product_epm_nested_source_witness_after(
        product_epm_modeled_empty_nested_source(),
    );
    let frame = state.bridge.admission.frames.last();
    assert(state.bridge.admission.frames.len() == 2);
    assert(frame.program_type_id == 31);
    assert(frame.frame_kind == 1);
    assert(frame.cursor == frame.region_end);
    assert(frame.waiting_id.is_none());
    assert(frame.score_handle.is_none());
    assert(state.epm_frames.last().is_none());
    assert(state.accepted_epm_sources.last().unwrap().epm.snapshot.bytes
        == product_epm_modeled_empty_snapshot_bytes());
    assert(state.bridge.transcript == rpb3_score_transcript::empty_arena());
    assert(state.bridge.admission.arena.len() == 0);
    assert(score_bridge::bridge_bool_slot(frame, 1, 4) == Some(false));
    assert(admission::admission_family_complete(frame));
    assert(admission::admission_slots_complete(frame));
    product_epm_empty_snapshot_selected_nonempty(state);
    assert(product_epm_source_child_score_selected(state).is_some());
}

// Keep the score append and the source-derived child frame facts in their own
// solver context.  The public readiness witness below combines these facts
// only after the score selector has been established.
pub proof fn product_epm_modeled_empty_nested_scored_child_shape()
    ensures {
        let product = product_epm_nested_source_witness_after(
            product_epm_modeled_empty_nested_source(),
        );
        let scored = product_epm_source_bridge_finish_selected(product).unwrap().0;
        &&& product_epm_source_bridge_finish_selected(product).is_some()
        &&& admission::admission_state_wf(scored.admission)
        &&& score_bridge::bridge_score_step_shape(product.bridge, scored)
        &&& scored.admission.frames[1].return_id.is_some()
        &&& admission::admission_epathmap_path_stability(
            scored.admission.frames[1],
        ) == Some(true)
    },
{
    product_epm_modeled_empty_nested_scores();
    let product = product_epm_nested_source_witness_after(
        product_epm_modeled_empty_nested_source(),
    );
    let scored = product_epm_source_bridge_finish_selected(product).unwrap().0;
    product_epm_source_bridge_finish_selected_exact(product);
    assert(score_bridge::bridge_score_step_shape(product.bridge, scored));
    assert(scored.admission.frames[1].return_id
        == product.bridge.admission.frames[1].return_id);
    assert(scored.admission.frames[1].returns
        == product.bridge.admission.frames[1].returns);
    assert(product_epm_modeled_empty_nested_source().epm.entries_stable);
    assert(admission::admission_epathmap_path_stability(
        scored.admission.frames[1],
    ) == Some(true));
}

pub proof fn product_epm_modeled_empty_nested_derived_summary()
    ensures {
        let product = product_epm_nested_source_witness_after(
            product_epm_modeled_empty_nested_source(),
        );
        let scored = product_epm_source_bridge_finish_selected(product).unwrap().0;
        &&& product_epm_source_bridge_finish_selected(product).is_some()
        &&& admission::concrete_derived_summary(
            scored.admission.frames[0], scored.admission.frames[1],
        ) == Some(admission::AdmissionReturnedKind::ExprPayload {
            score_present: true,
            canonical_fixed_point: true,
            canonical_path_stable: true,
        })
    },
{
    product_epm_modeled_empty_nested_scored_child_shape();
    let product = product_epm_nested_source_witness_after(
        product_epm_modeled_empty_nested_source(),
    );
    let scored = product_epm_source_bridge_finish_selected(product).unwrap().0;
    assert(admission::concrete_derived_summary(
        scored.admission.frames[0], scored.admission.frames[1],
    ) == Some(admission::AdmissionReturnedKind::ExprPayload {
        score_present: true,
        canonical_fixed_point: true,
        canonical_path_stable: true,
    }));
}

pub proof fn product_epm_modeled_empty_nested_complete_enabled()
    ensures {
        let product = product_epm_nested_source_witness_after(
            product_epm_modeled_empty_nested_source(),
        );
        let scored = product_epm_source_bridge_finish_selected(product).unwrap().0;
        &&& product_epm_source_bridge_finish_selected(product).is_some()
        &&& admission::admission_complete_child_step(
            scored.admission,
            scored.admission.frames[1].return_id.unwrap(),
            1,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: true,
                canonical_fixed_point: true,
                canonical_path_stable: true,
            },
        ).is_some()
    },
{
    product_epm_modeled_empty_nested_scored_child_shape();
    product_epm_modeled_empty_nested_derived_summary();
    let product = product_epm_nested_source_witness_after(
        product_epm_modeled_empty_nested_source(),
    );
    let scored = product_epm_source_bridge_finish_selected(product).unwrap().0;
    assert(admission::admission_complete_child_step(
        scored.admission,
        scored.admission.frames[1].return_id.unwrap(),
        1,
        admission::AdmissionReturnedKind::ExprPayload {
            score_present: true,
            canonical_fixed_point: true,
            canonical_path_stable: true,
        },
    ).is_some());
}

pub proof fn product_epm_modeled_empty_nested_complete_ready()
    ensures {
        let product = product_epm_nested_source_witness_after(
            product_epm_modeled_empty_nested_source(),
        );
        let scored = product_epm_source_bridge_finish_selected(product).unwrap().0;
        &&& product_epm_source_bridge_finish_selected(product).is_some()
        &&& scored.admission.frames[1].return_id.is_some()
        &&& admission::admission_epathmap_path_stability(
            scored.admission.frames[1],
        ) == Some(true)
        &&& admission::concrete_derived_summary(
            scored.admission.frames[0], scored.admission.frames[1],
        ) == Some(admission::AdmissionReturnedKind::ExprPayload {
            score_present: true,
            canonical_fixed_point: true,
            canonical_path_stable: true,
        })
        &&& admission::admission_complete_child_step(
            scored.admission,
            scored.admission.frames[1].return_id.unwrap(),
            1,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: true,
                canonical_fixed_point: true,
                canonical_path_stable: true,
            },
        ).is_some()
    },
{
    product_epm_modeled_empty_nested_scored_child_shape();
    product_epm_modeled_empty_nested_derived_summary();
    product_epm_modeled_empty_nested_complete_enabled();
}

pub proof fn product_epm_modeled_empty_nested_admission_complete_dispatch()
    ensures {
        let before = product_epm_nested_source_witness_after(
            product_epm_modeled_empty_nested_source(),
        );
        let scored = product_epm_source_child_score_selected(before).unwrap();
        &&& product_epm_source_child_score_selected(before).is_some()
        &&& product_score_wf(scored)
        &&& match admission::concrete_message_driver_step(
            scored.bridge.admission, admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { .. }) => true,
            _ => false,
        }
    },
{
    product_epm_modeled_empty_nested_complete_ready();
    product_epm_modeled_empty_nested_scores();
    let before = product_epm_nested_source_witness_after(
        product_epm_modeled_empty_nested_source(),
    );
    let scored = product_epm_source_child_score_selected(before).unwrap();
    product_epm_source_child_score_selected_exact(before);
    let finish = product_epm_source_bridge_finish_selected(before).unwrap().0;
    let summary = admission::AdmissionReturnedKind::ExprPayload {
        score_present: true,
        canonical_fixed_point: true,
        canonical_path_stable: true,
    };
    assert(scored.bridge == finish) by {
        assert(product_epm_source_child_score_selected(before).is_some());
    }
    assert(admission::concrete_derived_summary(
        scored.bridge.admission.frames[0], scored.bridge.admission.frames[1],
    ) == Some(summary));
    assert(admission::admission_complete_child_step(
        scored.bridge.admission,
        scored.bridge.admission.frames[1].return_id.unwrap(),
        1, summary,
    ).is_some());
    assert(match admission::concrete_message_driver_step(
        scored.bridge.admission, admission::ConcreteFrameActionModel::Complete,
    ) {
        Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { .. }) => true,
        _ => false,
    }) by {
        admission::admission_complete_child_preserves_wf(
            scored.bridge.admission,
            scored.bridge.admission.frames[1].return_id.unwrap(),
            1, summary,
        );
    }
}

pub proof fn product_epm_modeled_empty_nested_map_complete_dispatch()
    ensures {
        let before = product_epm_nested_source_witness_after(
            product_epm_modeled_empty_nested_source(),
        );
        let scored = product_epm_source_child_score_selected(before).unwrap();
        let action = map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        };
        &&& product_epm_source_child_score_selected(before).is_some()
        &&& product_map_pop_owned(scored)
        &&& match map::integrated_admission_driver_step(scored.map, action) {
            Some(map::IntegratedAdmissionOutcome::MessageParentAccepted { .. }) => true,
            _ => false,
        }
    },
{
    product_epm_modeled_empty_nested_admission_complete_dispatch();
    let before = product_epm_nested_source_witness_after(
        product_epm_modeled_empty_nested_source(),
    );
    let scored = product_epm_source_child_score_selected(before).unwrap();
    let action = map::IntegratedAdmissionAction::Message {
        action: admission::ConcreteFrameActionModel::Complete,
    };
    assert(match map::integrated_admission_driver_step(
        scored.map, action,
    ) {
        Some(map::IntegratedAdmissionOutcome::MessageParentAccepted { .. }) => true,
        _ => false,
    }) by {
        assert(product_score_wf(scored));
        assert(scored.map.admission == scored.bridge.admission);
        assert(scored.map.transcript == scored.bridge.transcript);
        assert(scored.map.map_frames == before.map.map_frames);
        assert(scored.map.map_frames == seq![None, None]);
    }
    assert(scored.epm_frames == before.epm_frames);
    assert(before.epm_frames == seq![None, None]);
    assert(product_map_pop_owned(scored));
}

pub proof fn product_epm_modeled_empty_nested_product_complete_dispatch()
    ensures product_epm_source_child_complete_selected(
        product_epm_nested_source_witness_after(
            product_epm_modeled_empty_nested_source(),
        ),
    ).is_some(),
{
    product_epm_modeled_empty_nested_map_complete_dispatch();
    let before = product_epm_nested_source_witness_after(
        product_epm_modeled_empty_nested_source(),
    );
    let scored = product_epm_source_child_score_selected(before).unwrap();
    let action = map::IntegratedAdmissionAction::Message {
        action: admission::ConcreteFrameActionModel::Complete,
    };
    if let Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
        state: next_map,
    }) = map::integrated_admission_driver_step(scored.map, action) {
        let candidate = ProductScoreState {
            bridge: score_bridge::ScoreAdmissionBridge {
                admission: next_map.admission,
                transcript: next_map.transcript,
            },
            map: next_map,
            epm_frames: scored.epm_frames.drop_last(),
            indexed_regions: scored.indexed_regions.drop_last(),
            accepted_epm_sources: Seq::empty(),
        };
        let next = ProductScoreState {
            accepted_epm_sources: product_epm_receipts_carried(scored, candidate),
            ..candidate
        };
        assert(product_map_step(scored, next, action)) by {
            let outcome = map::IntegratedAdmissionOutcome::MessageParentAccepted {
                state: next_map,
            };
            assert(map::integrated_nonterminal_state(outcome) == Some(next_map));
            assert(product_map_regions(scored, action, outcome)
                == Some(scored.indexed_regions.drop_last()));
            assert(product_map_epm_frames(scored, outcome)
                == Some(scored.epm_frames.drop_last()));
            assert(product_epm_receipts_carried(scored, candidate)
                == product_epm_receipts_carried(scored, next));
        }
        assert(product_epm_source_child_complete_selected(before) == Some(next)) by {
            assert(product_epm_source_child_score_selected(before) == Some(scored));
            assert(map::integrated_admission_driver_step(scored.map, action)
                == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
                    state: next_map,
                }));
        }
    }
    assert(product_epm_source_child_complete_selected(before).is_some());
}

pub proof fn product_epm_modeled_empty_nested_complete_poststate()
    ensures product_epm_source_child_complete_selected(
        product_epm_nested_source_witness_after(
            product_epm_modeled_empty_nested_source(),
        ),
    ).is_some(),
{
    product_epm_modeled_empty_nested_product_complete_dispatch();
}

pub proof fn runtime_epm_source_child_selected_complete(
    before: runtime_source::RuntimeFrameImage,
    snapshot_bytes: Seq<u8>,
)
    requires
        runtime_source::runtime_frame_image_wf(before),
        before.score.admission.frames.len() == 2,
        runtime_source::runtime_epm_source_score_finish_selected(
            before, snapshot_bytes,
        ).is_some(),
        before.score.admission.frames[0].family == admission::AdmissionFamily::Expr,
        before.score.admission.frames[0].waiting_slot == Some(0),
        before.score.admission.frames[0].waiting_tag == Some(32),
        admission::admission_epathmap_path_stability(
            runtime_source::runtime_epm_source_score_finish_selected(
                before, snapshot_bytes,
            ).unwrap().admission.frames[1],
        ) == Some(true),
        runtime_source::runtime_epm_source_score_finish_selected(
            before, snapshot_bytes,
        ).unwrap().admission.frames[1].return_id.is_some(),
        admission::admission_complete_child_step(
            runtime_source::runtime_epm_source_score_finish_selected(
                before, snapshot_bytes,
            ).unwrap().admission,
            runtime_source::runtime_epm_source_score_finish_selected(
                before, snapshot_bytes,
            ).unwrap().admission.frames[1].return_id.unwrap(),
            1,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: true,
                canonical_fixed_point: true,
                canonical_path_stable: true,
            },
        ).is_some(),
    ensures match runtime_source::runtime_epm_source_eof(before, snapshot_bytes) {
        Some(runtime_source::RuntimeMessageOutcome::ParentAccepted { image }) => {
            &&& image.score.admission.frames.len() == 1
            &&& image.score.admission.completed
                == before.score.admission.completed + 1
        },
        _ => false,
    },
{
    let scored = runtime_source::runtime_epm_source_score_finish_selected(
        before, snapshot_bytes,
    ).unwrap();
    runtime_source::runtime_epm_source_score_finish_is_checked(before, snapshot_bytes);
    assert(admission::admission_state_wf(scored.admission));
    assert(admission::concrete_derived_summary(
        scored.admission.frames[0], scored.admission.frames[1],
    ) == Some(admission::AdmissionReturnedKind::ExprPayload {
        score_present: true,
        canonical_fixed_point: true,
        canonical_path_stable: true,
    }));
    admission::admission_complete_child_preserves_wf(
        scored.admission,
        scored.admission.frames[1].return_id.unwrap(),
        1,
        admission::AdmissionReturnedKind::ExprPayload {
            score_present: true,
            canonical_fixed_point: true,
            canonical_path_stable: true,
        },
    );
    let pre_pop = runtime_source::RuntimeFrameImage {
        score: scored,
        ..before
    };
    assert(runtime_source::runtime_frame_image_wf(pre_pop));
    assert(match admission::concrete_message_driver_step(
        scored.admission, admission::ConcreteFrameActionModel::Complete,
    ) {
        Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { .. }) => true,
        _ => false,
    });
    reveal(runtime_source::runtime_pre_pop_child_accept);
    assert(runtime_source::runtime_pre_pop_child_accept(pre_pop).is_some());
    runtime_source::runtime_child_accept_is_derived(pre_pop);
}

pub proof fn live_runtime_epm_source_child_eof_accepts(
    before: LiveRuntimeProductState,
)
    requires
        live_runtime_product_wf(before),
        before.source.product.bridge.admission.frames.len() == 2,
        before.source.product.accepted_epm_sources.last().is_some(),
        before.source.product.bridge.admission.frames.last().cursor
            == before.source.product.bridge.admission.frames.last().region_end,
        before.cursors.last().is_some(),
        wire_source::wire_runtime_next_field(before.cursors.last().unwrap())
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    before.cursors.last().unwrap(),
                ).unwrap(),
            }),
        product_epm_source_bridge_finish_selected(before.source.product).is_some(),
        before.source.product.bridge.admission.frames[0].family
            == admission::AdmissionFamily::Expr,
        before.source.product.bridge.admission.frames[0].waiting_slot == Some(0),
        before.source.product.bridge.admission.frames[0].waiting_tag == Some(32),
        admission::admission_epathmap_path_stability(
            product_epm_source_bridge_finish_selected(before.source.product)
                .unwrap().0.admission.frames[1],
        ) == Some(true),
        product_epm_source_bridge_finish_selected(before.source.product)
            .unwrap().0.admission.frames[1].return_id.is_some(),
        admission::admission_complete_child_step(
            product_epm_source_bridge_finish_selected(before.source.product)
                .unwrap().0.admission,
            product_epm_source_bridge_finish_selected(before.source.product)
                .unwrap().0.admission.frames[1].return_id.unwrap(),
            1,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: true,
                canonical_fixed_point: true,
                canonical_path_stable: true,
            },
        ).is_some(),
    ensures match live_runtime_epm_source_eof_selected(before) {
        Some(runtime_source::RuntimeMessageOutcome::ParentAccepted { image }) => {
            &&& image.score.admission.frames.len() == 1
            &&& image.score.admission.completed
                == before.source.runtime.score.admission.completed + 1
        },
        _ => false,
    },
{
    live_runtime_epm_source_eof_dispatch_exact(before);
    let bytes = before.source.product.accepted_epm_sources.last().unwrap()
        .epm.snapshot.bytes;
    runtime_epm_source_child_selected_complete(before.source.runtime, bytes);
}

pub proof fn live_epm_modeled_empty_nested_eof_accepts()
    ensures match live_runtime_epm_source_eof_selected(
        live_epm_modeled_empty_nested_after_accept(),
    ) {
        Some(runtime_source::RuntimeMessageOutcome::ParentAccepted { image }) => {
            &&& image.score.admission.frames.len() == 1
            &&& image.score.admission.completed == 2
        },
        _ => false,
    },
{
    product_epm_modeled_empty_nested_complete_ready();
    live_epm_modeled_empty_nested_accept_witness();
    let before = live_epm_modeled_empty_nested_after_accept();
    live_runtime_epm_source_child_eof_accepts(before);
}

pub open spec fn live_runtime_epm_source_child_complete_selected(
    before: LiveRuntimeProductState,
) -> Option<LiveRuntimeProductState> {
    match (
        product_epm_source_child_complete_selected(before.source.product),
        live_runtime_epm_source_eof_selected(before),
    ) {
        (Some(product), Some(runtime_source::RuntimeMessageOutcome::ParentAccepted {
            image,
        })) if image.score == product.bridge => Some(LiveRuntimeProductState {
            source: LiveSourceProductState {
                product,
                runtime: image,
                arena: product_runtime_arena_from_score(product.bridge.transcript),
                wire: before.source.wire.drop_last(),
            },
            cursors: before.cursors.drop_last(),
        }),
        _ => None,
    }
}

pub proof fn live_runtime_epm_child_poststate_score_agrees(
    before: LiveRuntimeProductState,
)
    requires
        live_runtime_product_wf(before),
        before.source.product.bridge.admission.frames.len() == 2,
        before.source.product.accepted_epm_sources.last().is_some(),
        before.source.product.bridge.admission.frames.last().cursor
            == before.source.product.bridge.admission.frames.last().region_end,
        before.cursors.last().is_some(),
        wire_source::wire_runtime_next_field(before.cursors.last().unwrap())
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    before.cursors.last().unwrap(),
                ).unwrap(),
            }),
        product_epm_source_bridge_finish_selected(before.source.product).is_some(),
        product_epm_source_child_complete_selected(before.source.product).is_some(),
        match live_runtime_epm_source_eof_selected(before) {
            Some(runtime_source::RuntimeMessageOutcome::ParentAccepted { .. }) => true,
            _ => false,
        },
    ensures {
        let product = product_epm_source_child_complete_selected(
            before.source.product,
        ).unwrap();
        match live_runtime_epm_source_eof_selected(before) {
            Some(runtime_source::RuntimeMessageOutcome::ParentAccepted { image }) =>
                image.score == product.bridge,
            _ => false,
        }
    },
{
    live_runtime_epm_source_eof_dispatch_exact(before);
    let scored_product = product_epm_source_child_score_selected(
        before.source.product,
    ).unwrap();
    let product = product_epm_source_child_complete_selected(
        before.source.product,
    ).unwrap();
    let image = match live_runtime_epm_source_eof_selected(before) {
        Some(runtime_source::RuntimeMessageOutcome::ParentAccepted { image }) => image,
        _ => { assert(false); return; },
    };
    let scored_bridge = product_epm_source_bridge_finish_selected(
        before.source.product,
    ).unwrap().0;
    let pre_pop = runtime_source::RuntimeFrameImage {
        score: scored_bridge,
        ..before.source.runtime
    };
    assert(runtime_source::runtime_pre_pop_child_accept(pre_pop) == Some(image));
    reveal(runtime_source::runtime_pre_pop_child_accept);
    assert(scored_product.bridge == scored_bridge);
    assert(map::integrated_admission_driver_step(
        scored_product.map,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        },
    ) == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
        state: product.map,
    }));
    assert(image.score.admission == product.bridge.admission);
    assert(image.score.transcript == product.bridge.transcript);
}

pub proof fn live_runtime_epm_source_child_complete_selected_exact(
    before: LiveRuntimeProductState,
)
    ensures match live_runtime_epm_source_child_complete_selected(before) {
        None => true,
        Some(after) => {
            let scored = product_epm_source_child_score_selected(
                before.source.product,
            ).unwrap();
            &&& product_score_wf(after.source.product)
            &&& product_map_step(
                scored, after.source.product,
                map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::Complete,
                },
            )
            &&& after.source.runtime.score == after.source.product.bridge
            &&& arena_source::runtime_projects_score(
                after.source.arena, after.source.product.bridge.transcript,
            )
            &&& after.source.wire == before.source.wire.drop_last()
            &&& after.cursors == before.cursors.drop_last()
            &&& after.source.product.bridge.admission.frames.len() + 1
                == before.source.product.bridge.admission.frames.len()
        },
    },
{
    if let Some(after) = live_runtime_epm_source_child_complete_selected(before) {
        product_epm_source_child_complete_selected_exact(before.source.product);
        product_runtime_arena_from_score_projects(
            after.source.product.bridge.transcript,
        );
    }
}

pub proof fn live_source_product_epm_child_complete_preserves_wf(
    before: LiveSourceProductState,
    after: LiveSourceProductState,
    scored: ProductScoreState,
)
    requires
        live_source_product_wf(before),
        product_epm_source_child_score_selected(before.product) == Some(scored),
        product_score_wf(after.product),
        product_map_step(
            scored, after.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ),
        map::integrated_admission_driver_step(
            scored.map,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ) == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
            state: after.product.map,
        }),
        after.runtime.score == after.product.bridge,
        runtime_source::runtime_frame_image_wf(after.runtime),
        arena_source::runtime_projects_score(
            after.arena, after.product.bridge.transcript,
        ),
        after.wire == before.wire.drop_last(),
    ensures live_source_product_wf(after),
{
    product_epm_source_child_score_selected_exact(before.product);
    product_epm_source_bridge_finish_selected_exact(before.product);
    map::integrated_message_parent_accept_pop_shape(scored.map, after.product.map);
    assert(after.wire.len() == after.product.bridge.admission.frames.len());
    assert forall|i: int| 0 <= i < after.wire.len() implies {
        let frame = after.product.bridge.admission.frames[i];
        match #[trigger] after.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire_source::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    } by {
        assert(after.wire[i] == before.wire[i]);
        let old_frame = before.product.bridge.admission.frames[i];
        let scored_frame = scored.bridge.admission.frames[i];
        let new_frame = after.product.bridge.admission.frames[i];
        assert(scored_frame == old_frame);
        assert(new_frame.frame_kind == scored_frame.frame_kind);
        assert(new_frame.region_start == scored_frame.region_start);
        assert(new_frame.region_end == scored_frame.region_end);
        assert(new_frame.cursor == scored_frame.cursor);
        assert(new_frame.source_bytes == scored_frame.source_bytes);
    }
}

pub proof fn live_runtime_epm_child_complete_runtime_wf(
    before: LiveRuntimeProductState,
)
    requires
        live_runtime_product_wf(before),
        live_runtime_epm_source_child_complete_selected(before).is_some(),
    ensures runtime_source::runtime_frame_image_wf(
        live_runtime_epm_source_child_complete_selected(before)
            .unwrap().source.runtime,
    ),
{
    live_runtime_epm_source_child_complete_selected_exact(before);
    let after = live_runtime_epm_source_child_complete_selected(before).unwrap();
    let bytes = before.source.product.accepted_epm_sources.last().unwrap()
        .epm.snapshot.bytes;
    let scored = runtime_source::runtime_epm_source_score_finish_selected(
        before.source.runtime, bytes,
    ).unwrap();
    let pre_pop = runtime_source::RuntimeFrameImage {
        score: scored,
        ..before.source.runtime
    };
    reveal(runtime_source::runtime_pre_pop_child_accept);
    assert(runtime_source::runtime_pre_pop_child_accept(pre_pop)
        == Some(after.source.runtime));
    assert(after.source.runtime.capacity == before.source.runtime.capacity);
    assert(after.source.runtime.frame_bytes == before.source.runtime.frame_bytes);
    assert(after.source.runtime.max_frames == before.source.runtime.max_frames);
    assert(after.source.runtime.max_storage_bytes
        == before.source.runtime.max_storage_bytes);
    assert(after.source.runtime.max_capacity_bytes
        == before.source.runtime.max_capacity_bytes);
    assert(after.source.runtime.score.admission.frames.len() > 0);
    assert(after.source.runtime.score.admission.frames.len()
        <= after.source.runtime.capacity as int);
    assert(after.source.runtime.max_frames as int
        >= after.source.runtime.score.admission.frames.len());
    let old_len = before.source.runtime.score.admission.frames.len();
    let new_len = after.source.runtime.score.admission.frames.len();
    let frame_bytes = after.source.runtime.frame_bytes as int;
    let max_storage_bytes = after.source.runtime.max_storage_bytes as int;
    assert(max_storage_bytes >= old_len * frame_bytes);
    assert(new_len <= old_len);
    assert(frame_bytes > 0);
    assert(max_storage_bytes >= new_len * frame_bytes)
        by (nonlinear_arith)
        requires
            max_storage_bytes >= old_len * frame_bytes,
            new_len <= old_len,
            frame_bytes > 0;
}

pub proof fn live_runtime_epm_source_child_complete_preserves_wf(
    before: LiveRuntimeProductState,
)
    requires
        live_runtime_product_wf(before),
        live_runtime_epm_source_child_complete_selected(before).is_some(),
    ensures {
        let after = live_runtime_epm_source_child_complete_selected(before).unwrap();
        &&& live_runtime_product_wf(after)
        &&& after.source.runtime.score == after.source.product.bridge
        &&& after.source.product.bridge.admission.frames.len() + 1
            == before.source.product.bridge.admission.frames.len()
        &&& after.source.wire == before.source.wire.drop_last()
        &&& after.cursors == before.cursors.drop_last()
    },
{
    live_runtime_epm_source_child_complete_selected_exact(before);
    live_runtime_epm_child_complete_runtime_wf(before);
    let after = live_runtime_epm_source_child_complete_selected(before).unwrap();
    let scored = product_epm_source_child_score_selected(before.source.product).unwrap();
    assert(map::integrated_admission_driver_step(
        scored.map,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        },
    ) == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
        state: after.source.product.map,
    }));
    live_source_product_epm_child_complete_preserves_wf(
        before.source, after.source, scored,
    );
    live_runtime_wire_sources_pop(before, after);
    assert(after.cursors.len() == after.source.wire.len());
    assert forall|i: int| 0 <= i < after.cursors.len() implies {
        match (#[trigger] after.cursors[i], after.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(after.cursors[i] == before.cursors[i]);
        assert(after.source.wire[i] == before.source.wire[i]);
    }
}

pub proof fn live_epm_modeled_empty_nested_poststate_selected()
    ensures live_runtime_epm_source_child_complete_selected(
        live_epm_modeled_empty_nested_after_accept(),
    ).is_some(),
{
    product_epm_modeled_empty_nested_complete_poststate();
    live_epm_modeled_empty_nested_eof_accepts();
    live_epm_modeled_empty_nested_accept_witness();
    let before = live_epm_modeled_empty_nested_after_accept();
    live_runtime_epm_child_poststate_score_agrees(before);
}

pub proof fn live_epm_modeled_empty_nested_poststate_wf()
    ensures {
        let before = live_epm_modeled_empty_nested_after_accept();
        let after = live_runtime_epm_source_child_complete_selected(before).unwrap();
        &&& live_runtime_epm_source_child_complete_selected(before).is_some()
        &&& live_runtime_product_wf(after)
        &&& after.source.product.bridge.admission.frames.len() == 1
        &&& after.source.runtime.score == after.source.product.bridge
        &&& after.source.wire.len() == 1
        &&& after.cursors.len() == 1
    },
{
    live_epm_modeled_empty_nested_poststate_selected();
    live_epm_modeled_empty_nested_accept_witness();
    let before = live_epm_modeled_empty_nested_after_accept();
    live_runtime_epm_source_child_complete_preserves_wf(before);
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_child_cursor(
    parent: wire_source::WireRuntimeCursorImage,
    field: admission::IndexedCanonicalFieldModel,
) -> Option<wire_source::WireRuntimeCursorImage> {
    match wire_source::wire_runtime_subview(
        parent.message, field.payload_start, field.payload_end,
    ) {
        Some(backing) => wire_source::wire_runtime_initial(backing, false),
        None => None,
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_wire_sources_push(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    field: admission::IndexedCanonicalFieldModel,
)
    requires
        live_runtime_product_wf(before),
        before.cursors.len() > 0,
        before.cursors.last().is_some(),
        product_indexed_child(
            before.source.product.indexed_regions.last(), field,
        ).is_some(),
        live_runtime_child_cursor(before.cursors.last().unwrap(), field).is_some(),
        after.cursors.len() == before.cursors.len() + 1,
        after.source.product.indexed_regions
            == before.source.product.indexed_regions.push(
                product_indexed_child(
                    before.source.product.indexed_regions.last(), field,
                ).unwrap(),
            ),
        forall|i: int| 0 <= i < before.cursors.len() - 1
            ==> after.cursors[i] == before.cursors[i],
        after.cursors[before.cursors.len() - 1].is_some(),
        after.cursors[before.cursors.len() - 1].unwrap().message
            == before.cursors.last().unwrap().message,
        after.cursors.last()
            == live_runtime_child_cursor(before.cursors.last().unwrap(), field),
    ensures forall|i: int| 0 <= i < after.cursors.len()
        ==> match #[trigger] after.cursors[i] {
            None => true,
            Some(cursor) => product_wire_indexed_source_agrees(
                cursor, after.source.product.indexed_regions[i],
            ),
        },
{
    let parent = before.cursors.last().unwrap();
    let image = before.source.product.indexed_regions.last();
    product_wire_indexed_child_source_ids_exact(parent, image, field);
    assert forall|i: int| 0 <= i < after.cursors.len()
        implies match #[trigger] after.cursors[i] {
            None => true,
            Some(cursor) => product_wire_indexed_source_agrees(
                cursor, after.source.product.indexed_regions[i],
            ),
        } by {
        if i < before.cursors.len() - 1 {
            assert(after.cursors[i] == before.cursors[i]);
            assert(after.source.product.indexed_regions[i]
                == before.source.product.indexed_regions[i]);
        } else if i == before.cursors.len() - 1 {
            product_wire_indexed_source_agrees_same_backing(
                parent, after.cursors[i].unwrap(), image,
            );
        } else {
            assert(i == before.cursors.len());
            assert(after.cursors[i]
                == live_runtime_child_cursor(parent, field));
            assert(after.source.product.indexed_regions[i]
                == product_indexed_child(image, field).unwrap());
        }
    }
}

pub open spec fn live_runtime_product_continue_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    if !live_runtime_product_wf(before) || before.cursors.len() == 0 {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(cursor) => match wire_source::wire_runtime_next_field(cursor) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field, start: _,
                }) => {
                    let next = runtime_wire_after_field(cursor, concrete_after);
                    &&& live_source_product_continue_step(before.source, after.source)
                    &&& after.cursors == before.cursors.update(
                        before.cursors.len() - 1, Some(next),
                    )
                    &&& after.source.wire.last()
                        == wire_source::wire_runtime_cursor_project(next)
                },
                _ => false,
            },
        }
    }
}

pub open spec fn live_runtime_product_continue_indexed_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    if !live_runtime_product_wf(before) || before.cursors.len() == 0 {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(cursor) => match wire_source::wire_runtime_next_field(cursor) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field, start: _,
                }) => {
                    let next = runtime_wire_after_field(cursor, concrete_after);
                    &&& live_source_product_continue_indexed_step(
                        before.source, after.source,
                    )
                    &&& after.cursors == before.cursors.update(
                        before.cursors.len() - 1, Some(next),
                    )
                    &&& after.source.wire.last()
                        == wire_source::wire_runtime_cursor_project(next)
                },
                _ => false,
            },
        }
    }
}

pub proof fn live_runtime_product_continue_indexed_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_continue_indexed_step(before, after),
    ensures live_runtime_product_wf(after),
{
    live_source_product_continue_indexed_preserves_wf(before.source, after.source);
    live_runtime_wire_sources_steady(before, after);
    assert forall|i: int| 0 <= i < after.cursors.len() implies {
        match (#[trigger] after.cursors[i], after.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        if i < after.cursors.len() - 1 {
            assert(after.cursors[i] == before.cursors[i]);
            assert(after.source.wire[i] == before.source.wire[i]);
        }
    }
}

pub proof fn live_runtime_product_continue_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_continue_step(before, after),
    ensures live_runtime_product_wf(after),
{
    live_source_product_continue_preserves_wf(before.source, after.source);
    live_runtime_wire_sources_steady(before, after);
    assert forall|i: int| 0 <= i < after.cursors.len() implies {
        match (#[trigger] after.cursors[i], after.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        if i < after.cursors.len() - 1 {
            assert(after.cursors[i] == before.cursors[i]);
            assert(after.source.wire[i] == before.source.wire[i]);
        }
    }
}

pub open spec fn live_runtime_product_map_key_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    if !live_runtime_product_wf(before) || before.cursors.len() == 0
        || before.source.product.bridge.admission.frames.last().frame_kind != 2
    {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(cursor) => match wire_source::wire_runtime_next_field(cursor) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field: _, start: _,
                }) => {
                    let next = runtime_wire_after_field(cursor, concrete_after);
                    &&& live_source_product_map_key_step(before.source, after.source)
                    &&& after.cursors == before.cursors.update(
                        before.cursors.len() - 1, Some(next),
                    )
                    &&& after.source.wire.last()
                        == wire_source::wire_runtime_cursor_project(next)
                },
                _ => false,
            },
        }
    }
}

pub proof fn live_runtime_product_map_key_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_map_key_step(before, after),
    ensures live_runtime_product_wf(after),
{
    live_source_product_map_key_preserves_wf(before.source, after.source);
    live_runtime_wire_sources_steady(before, after);
    assert forall|i: int| 0 <= i < after.cursors.len() implies {
        match (#[trigger] after.cursors[i], after.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        if i < after.cursors.len() - 1 {
            assert(after.cursors[i] == before.cursors[i]);
            assert(after.source.wire[i] == before.source.wire[i]);
        }
    }
}

pub proof fn live_runtime_product_map_key_uses_read_field(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_map_key_step(before, after),
    ensures exists|cursor: wire_source::WireRuntimeCursorImage,
        concrete_after: wire_source::WireConcreteCursorImage,
        field: admission::IndexedCanonicalFieldModel,
        start: wire_source::WirePhysicalPosition| {
        &&& before.cursors.last() == Some(cursor)
        &&& wire_source::wire_runtime_next_field(cursor)
            == Some(wire_source::WireConcreteTransition::Field {
                state: concrete_after, field, start,
            })
        &&& product_map_step(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::MapKey { field },
        )
        &&& after.source.runtime.score == after.source.product.bridge
    },
{
    let cursor = before.cursors.last().unwrap();
    wire_source::wire_runtime_next_field_from_source(cursor);
    let reader = before.source.wire.last().unwrap();
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(cursor) {
        if let wire_source::WireSourceTransition::Field {
            state: _, field: source_field,
        } = wire_source::wire_source_next_field(reader) {
            assert(field == source_field);
            assert(product_map_step(
                before.source.product, after.source.product,
                map::IntegratedAdmissionAction::MapKey { field },
            ));
        }
    }
}

pub proof fn live_runtime_product_continue_uses_read_field(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_continue_step(before, after),
    ensures exists|cursor: wire_source::WireRuntimeCursorImage,
        concrete_after: wire_source::WireConcreteCursorImage,
        field: admission::IndexedCanonicalFieldModel,
        start: wire_source::WirePhysicalPosition| {
        &&& before.cursors.last() == Some(cursor)
        &&& wire_source::wire_runtime_next_field(cursor)
            == Some(wire_source::WireConcreteTransition::Field {
                state: concrete_after, field, start,
            })
        &&& product_map_step(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Continue { field },
            },
        )
        &&& runtime_source::runtime_continue(before.source.runtime, field)
            == Some(after.source.runtime)
    },
{
    let cursor = before.cursors.last().unwrap();
    wire_source::wire_runtime_next_field_from_source(cursor);
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(cursor) {
        let source_reader = before.source.wire.last().unwrap();
        if let wire_source::WireSourceTransition::Field {
            state: next_reader, field: source_field,
        } = wire_source::wire_source_next_field(source_reader) {
            assert(source_field == field);
            assert(product_map_step(
                before.source.product, after.source.product,
                map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::Continue { field },
                },
            ));
            assert(runtime_source::runtime_continue(before.source.runtime, field)
                == Some(after.source.runtime));
        }
    }
}

pub proof fn live_runtime_product_continue_indexed_uses_read_field(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_continue_indexed_step(before, after),
    ensures exists|cursor: wire_source::WireRuntimeCursorImage,
        concrete_after: wire_source::WireConcreteCursorImage,
        field: admission::IndexedCanonicalFieldModel,
        start: wire_source::WirePhysicalPosition| {
        &&& before.cursors.last() == Some(cursor)
        &&& wire_source::wire_runtime_next_field(cursor)
            == Some(wire_source::WireConcreteTransition::Field {
                state: concrete_after, field, start,
            })
        &&& product_map_step(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::ContinueIndexed {
                    field,
                    parent_source: product_admission_source_image(
                        before.source.product.indexed_regions.last(),
                    ),
                },
            },
        )
    },
{
    let cursor = before.cursors.last().unwrap();
    wire_source::wire_runtime_next_field_from_source(cursor);
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(cursor) {
        let reader = before.source.wire.last().unwrap();
        if let wire_source::WireSourceTransition::Field {
            state: _, field: source_field,
        } = wire_source::wire_source_next_field(reader) {
            assert(source_field == field);
            assert(product_map_step(
                before.source.product, after.source.product,
                map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::ContinueIndexed {
                        field,
                        parent_source: product_admission_source_image(
                            before.source.product.indexed_regions.last(),
                        ),
                    },
                },
            ));
        }
    }
}

pub proof fn live_runtime_product_continue_indexed_anchor_exact(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    cursor: wire_source::WireRuntimeCursorImage,
    concrete_after: wire_source::WireConcreteCursorImage,
    field: admission::IndexedCanonicalFieldModel,
    start: wire_source::WirePhysicalPosition,
)
    requires
        live_runtime_product_continue_indexed_step(before, after),
        before.cursors.last() == Some(cursor),
        wire_source::wire_runtime_next_field(cursor)
            == Some(wire_source::WireConcreteTransition::Field {
                state: concrete_after, field, start,
            }),
        product_map_step(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::ContinueIndexed {
                    field,
                    parent_source: product_admission_source_image(
                        before.source.product.indexed_regions.last(),
                    ),
                },
            },
        ),
    ensures {
        let parent_source = product_admission_source_image(
            before.source.product.indexed_regions.last(),
        );
        let occurrence = after.source.product.bridge.admission.frames.last()
            .scalar_occurrences.last();
        &&& parent_source.bytes
            == before.source.product.bridge.admission.frames.last().source_bytes
        &&& admission::admission_indexed_source_position(
            parent_source, field.encoded_start,
        ) == Some(admission::AdmissionSourceAnchor {
            source: start.physical.source as usize,
            offset: start.physical.offset,
        })
        &&& occurrence.physical_source == start.physical.source as usize
        &&& occurrence.physical_start == start.physical.offset as int
        &&& occurrence.tag == field.number
        &&& occurrence.bytes == field.payload_bytes
        &&& after.source.product.bridge.admission.frames.last().cursor
            == field.encoded_end
    },
{
    let image = before.source.product.indexed_regions.last();
    let parent_source = product_admission_source_image(image);
    assert(product_wire_indexed_source_agrees(cursor, image));
    product_wire_indexed_field_anchor_exact(
        cursor, image, concrete_after, field, start,
    );
    product_map_indexed_continue_anchor_exact(
        before.source.product, after.source.product, field, parent_source,
    );
}

pub proof fn live_runtime_product_root_end_is_ready(
    state: LiveRuntimeProductState,
)
    requires
        live_runtime_product_wf(state),
        state.cursors.len() == 1,
        state.source.product.bridge.admission.frames[0].frame_kind == 1,
        state.source.product.bridge.admission.frames[0].waiting_id.is_none(),
        state.source.product.bridge.admission.frames[0].score_handle.is_none(),
        state.cursors[0].is_some(),
        wire_source::wire_runtime_next_field(state.cursors[0].unwrap())
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    state.cursors[0].unwrap(),
                ).unwrap(),
            }),
    ensures
        finish::message_eof_ready(state.source.product.bridge),
        state.source.product.bridge.admission.frames[0].cursor
            == state.source.product.bridge.admission.root_end,
{
    let cursor = state.cursors[0].unwrap();
    wire_source::wire_runtime_end_is_exact(cursor);
    assert(state.source.wire[0]
        == wire_source::wire_runtime_cursor_project(cursor));
    assert(state.source.product.bridge.admission.frames[0].cursor
        == state.source.wire[0].unwrap().cursor.next);
    assert(state.source.product.bridge.admission.frames[0].region_end
        == state.source.wire[0].unwrap().cursor.region_end);
}

#[cfg(rpb3_terminal)]
pub proof fn live_runtime_root_complete_matches_source(
    before: LiveRuntimeProductState,
    scored: ProductScoreState,
    returned: runtime_source::RuntimeRootReturnImage,
    claimed: Seq<u8>,
)
    requires
        live_runtime_product_wf(before),
        before.cursors.len() == 1,
        before.cursors[0].is_some(),
        before.source.product.bridge.admission.frames[0].frame_kind == 1,
        before.source.product.bridge.admission.frames[0].waiting_id.is_none(),
        before.source.product.bridge.admission.frames[0].score_handle.is_none(),
        wire_source::wire_runtime_next_field(before.cursors[0].unwrap())
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    before.cursors[0].unwrap(),
                ).unwrap(),
            }),
        product_root_score_step(before.source.product, scored),
        score_bridge::bridge_score_finish_selected(before.source.runtime.score)
            == Some(scored.bridge),
        runtime_source::runtime_message_driver_step(
            before.source.runtime, runtime_source::RuntimeMessageEvent::Eof,
        ) == Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }),
    ensures
        finish::message_eof_ready(before.source.product.bridge),
        product_score_wf(scored),
        returned.pre_pop.score == scored.bridge,
        match map::integrated_admission_driver_step(
            scored.map,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ) {
            Some(map::IntegratedAdmissionOutcome::RootFinished { root_summary }) => {
                let handle = root_summary.score_handle;
                &&& root_summary == returned.summary
                &&& returned.score_bytes
                    == scored.bridge.admission.arena[handle as int].transcript_bytes
                &&& admission::admission_score_origin_matches(
                    scored.bridge.admission.frames[0],
                    scored.bridge.admission.arena,
                    handle,
                )
                &&& scored.bridge.admission.arena[handle as int].semantic
                    == admission::admission_family_complete(
                        scored.bridge.admission.frames[0],
                    )
                &&& scored.bridge.admission.arena[handle as int].transcript_bytes
                    == scored.bridge.transcript.entries[handle as int].transcript
                &&& score_bridge::bridge_root_first_difference(
                    scored.bridge, claimed,
                ).is_none() == rpb3_score_transcript::claimed_score_matches(
                    scored.bridge.transcript, handle, claimed,
                )
            },
            _ => false,
        },
{
    live_runtime_product_root_end_is_ready(before);
    product_root_score_step_preserves_exact(before.source.product, scored);
    assert(runtime_source::runtime_frame_image_wf(before.source.runtime));
    runtime_source::runtime_selected_eof_outcome_is_checked(before.source.runtime);
    assert(runtime_source::runtime_message_eof_root_return(
        before.source.runtime, scored.bridge,
    ) == Some(returned));
    runtime_source::runtime_root_return_is_pre_pop_exact(
        before.source.runtime, scored.bridge,
    );
    product_root_finish_claim_exact(scored, claimed);
    map::integrated_root_finish_returns_derived_summary(scored.map);
    if let Some(map::IntegratedAdmissionOutcome::RootFinished { root_summary }) =
        map::integrated_admission_driver_step(
            scored.map,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        )
    {
        assert(admission::admission_complete_root(scored.bridge.admission));
        score_bridge::bridge_root_exact(scored.bridge);
        assert(score_bridge::bridge_root_handle(scored.bridge)
            == Some(root_summary.score_handle));
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_child_cursor_is_payload(
    parent: wire_source::WireRuntimeCursorImage,
    concrete_after: wire_source::WireConcreteCursorImage,
    field: admission::IndexedCanonicalFieldModel,
    start: wire_source::WirePhysicalPosition,
)
    requires
        wire_source::wire_runtime_derive_cursor(parent).is_some(),
        wire_source::wire_concrete_cursor_wf(
            wire_source::wire_runtime_derive_cursor(parent).unwrap(),
        ),
        wire_source::wire_runtime_next_field(parent)
            == Some(wire_source::WireConcreteTransition::Field {
                state: concrete_after, field, start,
            }),
    ensures
        live_runtime_child_cursor(parent, field).is_some(),
        wire_source::wire_runtime_cursor_project(
            live_runtime_child_cursor(parent, field).unwrap(),
        ).is_some(),
        wire_source::wire_runtime_cursor_project(
            live_runtime_child_cursor(parent, field).unwrap(),
        ).unwrap().cursor.source_bytes == field.payload_bytes,
        wire_source::wire_runtime_cursor_project(
            live_runtime_child_cursor(parent, field).unwrap(),
        ).unwrap().cursor.next == 0,
{
    wire_source::wire_runtime_next_field_from_source(parent);
    let source = wire_source::wire_runtime_cursor_project(parent).unwrap();
    wire_source::wire_source_next_field_refines(source);
    let region = wire_source::wire_runtime_project_region(parent.message).unwrap();
    assert(admission::indexed_field_at_cursor(source.cursor, field));
    assert(field.payload_end as int <= region.bytes.len());
    wire_source::wire_runtime_subview_exact(
        parent.message, field.payload_start, field.payload_end,
    );
    let backing = wire_source::wire_runtime_subview(
        parent.message, field.payload_start, field.payload_end,
    ).unwrap();
    assert(wire_source::wire_runtime_project_region(backing).is_some());
    assert(wire_source::wire_runtime_project_region(backing).unwrap().bytes
        =~= field.payload_bytes);
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_parent_cursor_after_field_projects(
    parent: wire_source::WireRuntimeCursorImage,
    concrete_after: wire_source::WireConcreteCursorImage,
    field: admission::IndexedCanonicalFieldModel,
    start: wire_source::WirePhysicalPosition,
)
    requires
        wire_source::wire_runtime_derive_cursor(parent).is_some(),
        wire_source::wire_concrete_cursor_wf(
            wire_source::wire_runtime_derive_cursor(parent).unwrap(),
        ),
        wire_source::wire_runtime_next_field(parent)
            == Some(wire_source::WireConcreteTransition::Field {
                state: concrete_after, field, start,
            }),
    ensures exists|source_after: wire_source::WireSourceState| {
        &&& wire_source::wire_source_next_field(
            wire_source::wire_runtime_cursor_project(parent).unwrap(),
        ) == wire_source::WireSourceTransition::Field {
            state: source_after, field,
        }
        &&& wire_source::wire_runtime_cursor_project(
            runtime_wire_after_field(parent, concrete_after),
        ) == Some(source_after)
    },
{
    wire_source::wire_runtime_next_field_from_source(parent);
    let source_before = wire_source::wire_runtime_cursor_project(parent).unwrap();
    wire_source::wire_source_next_field_refines(source_before);
    if let wire_source::WireSourceTransition::Field {
        state: source_after, field: source_field,
    } = wire_source::wire_source_next_field(source_before) {
        assert(source_field == field);
        assert(wire_source::wire_runtime_cursor_project(
            runtime_wire_after_field(parent, concrete_after),
        ) == Some(source_after));
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn runtime_map_value_push_image(
    before: runtime_source::RuntimeFrameImage,
    after_score: score_bridge::ScoreAdmissionBridge,
    storage: runtime_source::SourceFramePushStorageImage,
) -> Option<runtime_source::RuntimeFrameImage> {
    if !runtime_source::source_successful_push_storage(before, storage)
        || !score_bridge::bridge_well_formed(after_score)
        || !admission::admission_state_wf(after_score.admission)
        || after_score.admission.frames.len() != storage.after_len as int
        || after_score.transcript != before.score.transcript
    {
        None
    } else {
        Some(runtime_source::RuntimeFrameImage {
            score: after_score,
            capacity: storage.after_capacity,
            frame_bytes: before.frame_bytes,
            max_frames: if before.max_frames >= storage.after_len {
                before.max_frames
            } else { storage.after_len },
            max_storage_bytes: if before.max_storage_bytes >= storage.used_bytes {
                before.max_storage_bytes
            } else { storage.used_bytes },
            max_capacity_bytes: if before.max_capacity_bytes >= storage.reserved_bytes {
                before.max_capacity_bytes
            } else { storage.reserved_bytes },
        })
    }
}

#[cfg(rpb3_live_push)]
pub proof fn runtime_map_value_push_image_is_bounded(
    before: runtime_source::RuntimeFrameImage,
    after_score: score_bridge::ScoreAdmissionBridge,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires runtime_map_value_push_image(before, after_score, storage).is_some(),
    ensures runtime_source::runtime_frame_image_wf(
        runtime_map_value_push_image(before, after_score, storage).unwrap(),
    ),
{}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_map_value_push_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
) -> bool {
    if !live_runtime_product_wf(before) || before.cursors.len() == 0
        || before.source.product.bridge.admission.frames.last().frame_kind != 2
    {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(parent) => match wire_source::wire_runtime_next_field(parent) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field, start: _,
                }) => match live_runtime_child_cursor(parent, field) {
                    None => false,
                    Some(child) => {
                        let next_parent = runtime_wire_after_field(parent, concrete_after);
                        match (
                            wire_source::wire_runtime_cursor_project(next_parent),
                            wire_source::wire_runtime_cursor_project(child),
                        ) {
                            (Some(next_parent_reader), Some(child_reader)) => {
                                &&& request.field == field
                                &&& request.child.frame_kind == 1
                                &&& request.child.source_bytes == field.payload_bytes
                                &&& runtime_source::source_frame_request_kind_code(
                                    request.request_kind,
                                ) == 1
                                &&& runtime_source::source_frame_request_kind_code(
                                    storage.request_kind,
                                ) == 1
                                &&& product_map_step(
                                    before.source.product, after.source.product,
                                    map::IntegratedAdmissionAction::MapValue {
                                        field,
                                        child: request.child,
                                        frame_bytes: before.source.runtime.frame_bytes,
                                    },
                                )
                                &&& runtime_map_value_push_image(
                                    before.source.runtime,
                                    after.source.product.bridge,
                                    storage,
                                ) == Some(after.source.runtime)
                                &&& after.source.arena == before.source.arena
                                &&& after.source.wire == before.source.wire.update(
                                    before.source.wire.len() - 1, Some(next_parent_reader),
                                ).push(Some(child_reader))
                                &&& after.cursors == before.cursors.update(
                                    before.cursors.len() - 1, Some(next_parent),
                                ).push(Some(child))
                            },
                            _ => false,
                        }
                    },
                },
                _ => false,
            },
        }
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_map_value_push_indexed_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
) -> bool {
    if !live_runtime_product_wf(before)
        || !product_frame_source_anchors_match(before.source.product)
        || before.cursors.len() == 0
        || before.source.product.indexed_regions.len() == 0
        || before.source.product.bridge.admission.frames.last().frame_kind != 2
    {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(parent) => match wire_source::wire_runtime_next_field(parent) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field, start: _,
                }) => match live_runtime_child_cursor(parent, field) {
                    None => false,
                    Some(child) => {
                        let parent_source = product_admission_source_image(
                            before.source.product.indexed_regions.last(),
                        );
                        let next_parent = runtime_wire_after_field(parent, concrete_after);
                        match (
                            wire_source::wire_runtime_cursor_project(next_parent),
                            wire_source::wire_runtime_cursor_project(child),
                        ) {
                            (Some(next_parent_reader), Some(child_reader)) => {
                                &&& product_wire_indexed_source_agrees(
                                    parent, before.source.product.indexed_regions.last(),
                                )
                                &&& request.field == field
                                &&& request.child.frame_kind == 1
                                &&& request.child.source_bytes == field.payload_bytes
                                &&& runtime_source::source_frame_request_kind_code(
                                    request.request_kind,
                                ) == 1
                                &&& runtime_source::source_frame_request_kind_code(
                                    storage.request_kind,
                                ) == 1
                                &&& product_map_step(
                                    before.source.product, after.source.product,
                                    map::IntegratedAdmissionAction::MapValueIndexed {
                                        field,
                                        child: request.child,
                                        parent_source,
                                        frame_bytes: before.source.runtime.frame_bytes,
                                    },
                                )
                                &&& runtime_map_value_push_image(
                                    before.source.runtime,
                                    after.source.product.bridge,
                                    storage,
                                ) == Some(after.source.runtime)
                                &&& after.source.arena == before.source.arena
                                &&& after.source.wire == before.source.wire.update(
                                    before.source.wire.len() - 1, Some(next_parent_reader),
                                ).push(Some(child_reader))
                                &&& after.cursors == before.cursors.update(
                                    before.cursors.len() - 1, Some(next_parent),
                                ).push(Some(child))
                            },
                            _ => false,
                        }
                    },
                },
                _ => false,
            },
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_map_value_push_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_map_value_push_step(before, after, request, storage),
    ensures live_runtime_product_wf(after),
{
    let parent = before.cursors.last().unwrap();
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(parent) {
        product_map_step_preserves_exact(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::MapValue {
                field,
                child: request.child,
                frame_bytes: before.source.runtime.frame_bytes,
            },
        );
        runtime_map_value_push_image_is_bounded(
            before.source.runtime, after.source.product.bridge, storage,
        );
        live_runtime_parent_cursor_after_field_projects(
            parent, concrete_after, field, start,
        );
        live_runtime_child_cursor_is_payload(parent, concrete_after, field, start);
        live_runtime_wire_sources_push(before, after, field);
        assert(after.source.product.bridge.transcript
            == before.source.product.bridge.transcript);
        assert(arena_source::runtime_projects_score(
            after.source.arena, after.source.product.bridge.transcript,
        ));
        assert forall|i: int| 0 <= i < after.source.wire.len() implies {
            let frame = after.source.product.bridge.admission.frames[i];
            match #[trigger] after.source.wire[i] {
                None => frame.frame_kind == 3 || frame.frame_kind == 4,
                Some(reader) => {
                    &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                    &&& wire_source::wire_cursor_shape(reader.cursor)
                    &&& reader.cursor.region_start == frame.region_start
                    &&& reader.cursor.region_end == frame.region_end
                    &&& reader.cursor.next == frame.cursor
                    &&& reader.cursor.source_bytes == frame.source_bytes
                    &&& reader.terminal_error.is_none()
                },
            }
        } by {
            if i < before.source.wire.len() - 1 {
                assert(after.source.wire[i] == before.source.wire[i]);
                assert(after.source.product.bridge.admission.frames[i]
                    == before.source.product.bridge.admission.frames[i]);
            }
        }
        assert forall|i: int| 0 <= i < after.cursors.len() implies {
            match (#[trigger] after.cursors[i], after.source.wire[i]) {
                (None, None) => true,
                (Some(cursor), Some(reader)) => {
                    &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                    &&& wire_source::wire_concrete_cursor_wf(
                        wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                    )
                    &&& wire_source::wire_runtime_cursor_project(cursor)
                        == Some(reader)
                },
                _ => false,
            }
        } by {
            if i < before.cursors.len() - 1 {
                assert(after.cursors[i] == before.cursors[i]);
                assert(after.source.wire[i] == before.source.wire[i]);
            }
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_map_value_push_indexed_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_map_value_push_indexed_step(
        before, after, request, storage,
    ),
    ensures live_runtime_product_wf(after),
{
    let parent = before.cursors.last().unwrap();
    let parent_source = product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(parent) {
        product_map_step_preserves_exact(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::MapValueIndexed {
                field,
                child: request.child,
                parent_source,
                frame_bytes: before.source.runtime.frame_bytes,
            },
        );
        runtime_map_value_push_image_is_bounded(
            before.source.runtime, after.source.product.bridge, storage,
        );
        live_runtime_parent_cursor_after_field_projects(
            parent, concrete_after, field, start,
        );
        live_runtime_child_cursor_is_payload(parent, concrete_after, field, start);
        live_runtime_wire_sources_push(before, after, field);
        assert(after.source.product.bridge.transcript
            == before.source.product.bridge.transcript);
        assert(arena_source::runtime_projects_score(
            after.source.arena, after.source.product.bridge.transcript,
        ));
        assert forall|i: int| 0 <= i < after.source.wire.len() implies {
            let frame = after.source.product.bridge.admission.frames[i];
            match #[trigger] after.source.wire[i] {
                None => frame.frame_kind == 3 || frame.frame_kind == 4,
                Some(reader) => {
                    &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                    &&& wire_source::wire_cursor_shape(reader.cursor)
                    &&& reader.cursor.region_start == frame.region_start
                    &&& reader.cursor.region_end == frame.region_end
                    &&& reader.cursor.next == frame.cursor
                    &&& reader.cursor.source_bytes == frame.source_bytes
                    &&& reader.terminal_error.is_none()
                },
            }
        } by {
            if i < before.source.wire.len() - 1 {
                assert(after.source.wire[i] == before.source.wire[i]);
                assert(after.source.product.bridge.admission.frames[i]
                    == before.source.product.bridge.admission.frames[i]);
            }
        }
        assert forall|i: int| 0 <= i < after.cursors.len() implies {
            match (#[trigger] after.cursors[i], after.source.wire[i]) {
                (None, None) => true,
                (Some(cursor), Some(reader)) => {
                    &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                    &&& wire_source::wire_concrete_cursor_wf(
                        wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                    )
                    &&& wire_source::wire_runtime_cursor_project(cursor)
                        == Some(reader)
                },
                _ => false,
            }
        } by {
            if i < before.cursors.len() - 1 {
                assert(after.cursors[i] == before.cursors[i]);
                assert(after.source.wire[i] == before.source.wire[i]);
            }
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_map_value_push_indexed_source_ids_match(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_map_value_push_indexed_step(
        before, after, request, storage,
    ),
    ensures product_wire_indexed_source_agrees(
        after.cursors.last().unwrap(),
        after.source.product.indexed_regions.last(),
    ),
    after.source.product.bridge.admission.frames.last().physical_source
        == product_indexed_start_anchor(
            after.source.product.indexed_regions.last(),
        ).source,
{
    let parent = before.cursors.last().unwrap();
    let image = before.source.product.indexed_regions.last();
    if let Some(wire_source::WireConcreteTransition::Field {
        field, ..
    }) = wire_source::wire_runtime_next_field(parent) {
        assert(product_indexed_child(image, field).is_some());
        product_wire_indexed_child_source_ids_exact(parent, image, field);
        product_map_value_indexed_push_anchor_exact(
            before.source.product, after.source.product,
            field, request.child, product_admission_source_image(image),
            before.source.runtime.frame_bytes,
        );
        assert(after.cursors.last() == live_runtime_child_cursor(parent, field));
        assert(after.source.product.indexed_regions.last()
            == product_indexed_child(image, field).unwrap());
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_map_value_push_observed_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
) -> bool {
    if !live_runtime_product_wf(before)
        || before.source.product.bridge.admission.frames.len() > usize::MAX as int
        || stats_frame_variant_bytes != before.source.runtime.frame_bytes
    {
        false
    } else {
        match runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            before.source.product.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
            runtime_source::SourceFramePushObservationResult::Success { storage } =>
                live_runtime_product_map_value_push_step(
                    before, after, request, storage,
                ) || live_runtime_product_map_value_push_indexed_step(
                    before, after, request, storage,
                ),
            _ => false,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_map_value_push_observed_is_exact(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires live_runtime_product_map_value_push_observed_step(
        before, after, request, actual_top_kind,
        after_len, after_capacity, stats_frame_variant_bytes,
    ),
    ensures
        live_runtime_product_wf(after),
        after.source.runtime.score.admission.frames.len() == after_len as int,
        after.source.runtime.capacity == after_capacity,
{
    if let runtime_source::SourceFramePushObservationResult::Success { storage } =
        runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            before.source.product.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
        runtime_source::source_frame_push_observed_storage_is_checked(
            before.source.runtime, request.request_kind,
            actual_top_kind, after_len, after_capacity,
        );
        if live_runtime_product_map_value_push_step(before, after, request, storage) {
            live_runtime_product_map_value_push_preserves_wf(
                before, after, request, storage,
            );
        } else {
            live_runtime_product_map_value_push_indexed_preserves_wf(
                before, after, request, storage,
            );
        }
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_map_value_accept_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    if !live_runtime_product_wf(before) || before.cursors.len() <= 1
        || before.source.product.bridge.admission.frames.last().frame_kind != 1
        || before.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 2
        ].frame_kind != 2
    {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(cursor) => {
                &&& wire_source::wire_runtime_next_field(cursor)
                    == Some(wire_source::WireConcreteTransition::End {
                        state: wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                    })
                &&& product_map_step(
                    before.source.product, after.source.product,
                    map::IntegratedAdmissionAction::MapAccept,
                )
                &&& after.source.runtime
                    == runtime_source::runtime_after_admission_step(
                        before.source.runtime,
                        after.source.product.bridge.admission,
                    )
                &&& after.source.arena == before.source.arena
                &&& after.source.wire == before.source.wire.drop_last()
                &&& after.cursors == before.cursors.drop_last()
            },
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_map_value_accept_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_map_value_accept_step(before, after),
    ensures
        live_runtime_product_wf(after),
        after.source.product.bridge.admission.frames.len() + 1
            == before.source.product.bridge.admission.frames.len(),
        after.source.product.bridge.admission.completed as int
            == before.source.product.bridge.admission.completed as int + 1,
{
    let cursor = before.cursors.last().unwrap();
    wire_source::wire_runtime_end_is_exact(cursor);
    product_map_step_preserves_exact(
        before.source.product, after.source.product,
        map::IntegratedAdmissionAction::MapAccept,
    );
    live_runtime_wire_sources_pop(before, after);
    assert(after.source.product.bridge.transcript
        == before.source.product.bridge.transcript);
    assert(runtime_source::runtime_frame_image_wf(after.source.runtime));
    assert(arena_source::runtime_projects_score(
        after.source.arena, after.source.product.bridge.transcript,
    ));
    assert forall|i: int| 0 <= i < after.source.wire.len() implies {
        let frame = after.source.product.bridge.admission.frames[i];
        match #[trigger] after.source.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire_source::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    } by {
        if i < after.source.wire.len() - 1 {
            assert(after.source.wire[i] == before.source.wire[i]);
            assert(after.source.product.bridge.admission.frames[i]
                == before.source.product.bridge.admission.frames[i]);
        }
    }
    assert forall|i: int| 0 <= i < after.cursors.len() implies {
        match (#[trigger] after.cursors[i], after.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(after.cursors[i] == before.cursors[i]);
        assert(after.source.wire[i] == before.source.wire[i]);
    }
}

#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn live_runtime_product_map_entry_finish_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    if !live_runtime_product_wf(before) || before.cursors.len() <= 1
        || before.source.product.bridge.admission.frames.last().frame_kind != 2
    {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(cursor) => {
                &&& wire_source::wire_runtime_next_field(cursor)
                    == Some(wire_source::WireConcreteTransition::End {
                        state: wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                    })
                &&& product_map_step(
                    before.source.product, after.source.product,
                    map::IntegratedAdmissionAction::MapFinish,
                )
                &&& after.source.runtime
                    == runtime_source::runtime_after_admission_step(
                        before.source.runtime,
                        after.source.product.bridge.admission,
                    )
                &&& after.source.arena == before.source.arena
                &&& after.source.wire == before.source.wire.drop_last()
                &&& after.cursors == before.cursors.drop_last()
            },
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_map_entry_finish_source_shape(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_map_entry_finish_step(before, after),
    ensures
        live_runtime_product_wf(before),
        before.cursors.len() > 1,
        before.cursors.last().is_some(),
        product_map_step(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::MapFinish,
        ),
        after.source.runtime == runtime_source::runtime_after_admission_step(
            before.source.runtime, after.source.product.bridge.admission,
        ),
        after.source.arena == before.source.arena,
        after.source.wire == before.source.wire.drop_last(),
        after.cursors == before.cursors.drop_last(),
{
    reveal(live_runtime_product_map_entry_finish_step);
}

#[cfg(rpb3_live_push)]
pub proof fn live_source_product_map_entry_finish_preserves_wf(
    before: LiveSourceProductState,
    after: LiveSourceProductState,
    summary: map::MapSummaryModel,
    next: map::IntegratedAdmissionState,
)
    requires
        live_source_product_wf(before),
        product_score_wf(after.product),
        product_map_step(
            before.product, after.product,
            map::IntegratedAdmissionAction::MapFinish,
        ),
        map::integrated_admission_driver_step(
            before.product.map, map::IntegratedAdmissionAction::MapFinish,
        ) == Some(map::IntegratedAdmissionOutcome::MapEntryCompleted {
            summary, state: next,
        }),
        after.runtime == runtime_source::runtime_after_admission_step(
            before.runtime, after.product.bridge.admission,
        ),
        after.arena == before.arena,
        after.wire == before.wire.drop_last(),
    ensures live_source_product_wf(after),
{
    map::integrated_map_finish_pop_shape(before.product.map, next, summary);
    assert(after.product.map == next);
    assert(after.product.bridge.admission == next.admission);
    assert(after.product.bridge.transcript == next.transcript);
    assert(after.product.bridge.transcript == before.product.bridge.transcript);
    assert(after.runtime.score == after.product.bridge);
    assert(after.product.bridge.admission.frames.len() + 1
        == before.product.bridge.admission.frames.len());
    assert(admission::admission_state_wf(after.runtime.score.admission));
    assert(score_bridge::bridge_well_formed(after.runtime.score));
    assert(after.runtime.score.admission.frames.len() > 0);
    assert(after.runtime.score.admission.frames.len()
        <= before.runtime.score.admission.frames.len());
    assert(after.runtime.capacity == before.runtime.capacity);
    assert(after.runtime.max_frames == before.runtime.max_frames);
    assert(after.runtime.max_storage_bytes == before.runtime.max_storage_bytes);
    assert(after.runtime.frame_bytes == before.runtime.frame_bytes);
    assert(after.runtime.max_capacity_bytes == before.runtime.max_capacity_bytes);
    assert(after.runtime.frame_bytes > 0);
    assert(after.runtime.score.admission.frames.len() <= after.runtime.capacity as int);
    assert(after.runtime.capacity as int * after.runtime.frame_bytes as int
        <= usize::MAX as int);
    assert(after.runtime.max_frames as int
        >= after.runtime.score.admission.frames.len());
    let old_len = before.runtime.score.admission.frames.len();
    let new_len = after.runtime.score.admission.frames.len();
    let frame_bytes = after.runtime.frame_bytes as int;
    let max_storage_bytes = after.runtime.max_storage_bytes as int;
    assert(max_storage_bytes >= old_len * frame_bytes);
    assert(new_len <= old_len);
    assert(frame_bytes > 0);
    assert(max_storage_bytes >= new_len * frame_bytes)
        by (nonlinear_arith)
        requires
            max_storage_bytes >= old_len * frame_bytes,
            new_len <= old_len,
            frame_bytes > 0;
    assert(after.runtime.max_capacity_bytes as int
        >= after.runtime.capacity as int * after.runtime.frame_bytes as int);
    assert(runtime_source::runtime_frame_image_wf(after.runtime));
    assert(arena_source::runtime_projects_score(
        after.arena, after.product.bridge.transcript,
    ));
    assert(after.wire.len() == after.product.bridge.admission.frames.len());
    assert forall|i: int| 0 <= i < after.wire.len() implies {
        let frame = after.product.bridge.admission.frames[i];
        match #[trigger] after.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire_source::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    } by {
        assert(after.wire[i] == before.wire[i]);
        if i < after.wire.len() - 1 {
            assert(after.product.bridge.admission.frames[i]
                == before.product.bridge.admission.frames[i]);
        } else {
            assert(i == after.product.bridge.admission.frames.len() - 1);
            assert(i == before.product.bridge.admission.frames.len() - 2);
            assert(after.product.bridge.admission.frames[i].frame_kind
                == before.product.bridge.admission.frames[i].frame_kind);
            assert(after.product.bridge.admission.frames[i].region_start
                == before.product.bridge.admission.frames[i].region_start);
            assert(after.product.bridge.admission.frames[i].region_end
                == before.product.bridge.admission.frames[i].region_end);
            assert(after.product.bridge.admission.frames[i].cursor
                == before.product.bridge.admission.frames[i].cursor);
            assert(after.product.bridge.admission.frames[i].source_bytes
                == before.product.bridge.admission.frames[i].source_bytes);
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_map_entry_finish_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_map_entry_finish_step(before, after),
    ensures
        live_runtime_product_wf(after),
        after.source.product.bridge.admission.frames.len() + 1
            == before.source.product.bridge.admission.frames.len(),
        after.source.product.bridge.admission.completed as int
            == before.source.product.bridge.admission.completed as int + 1,
{
    live_runtime_product_map_entry_finish_source_shape(before, after);
    let cursor = before.cursors.last().unwrap();
    wire_source::wire_runtime_end_is_exact(cursor);
    product_map_step_preserves_exact(
        before.source.product, after.source.product,
        map::IntegratedAdmissionAction::MapFinish,
    );
    live_runtime_wire_sources_pop(before, after);
    match map::integrated_admission_driver_step(
        before.source.product.map, map::IntegratedAdmissionAction::MapFinish,
    ) {
        Some(map::IntegratedAdmissionOutcome::MapEntryCompleted { summary, state: next }) => {
            live_source_product_map_entry_finish_preserves_wf(
                before.source, after.source, summary, next,
            );
            map::integrated_map_finish_pop_shape(before.source.product.map, next, summary);
        },
        _ => assert(false),
    }
    assert(after.cursors.len() == after.source.wire.len());
    assert forall|i: int| 0 <= i < after.cursors.len() implies {
        match (#[trigger] after.cursors[i], after.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(after.cursors[i] == before.cursors[i]);
        assert(after.source.wire[i] == before.source.wire[i]);
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_message_child_complete_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    if !live_runtime_product_wf(before) || before.cursors.len() <= 1
        || before.source.product.bridge.admission.frames.last().frame_kind != 1
        || before.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 2
        ].frame_kind != 1
    {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(cursor) => {
                &&& wire_source::wire_runtime_next_field(cursor)
                    == Some(wire_source::WireConcreteTransition::End {
                        state: wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                    })
                &&& product_map_step(
                    before.source.product, after.source.product,
                    map::IntegratedAdmissionAction::Message {
                        action: admission::ConcreteFrameActionModel::Complete,
                    },
                )
                &&& after.source.runtime
                    == runtime_source::runtime_after_admission_step(
                        before.source.runtime,
                        after.source.product.bridge.admission,
                    )
                &&& after.source.arena == before.source.arena
                &&& after.source.wire == before.source.wire.drop_last()
                &&& after.cursors == before.cursors.drop_last()
            },
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_source_product_message_child_complete_preserves_wf(
    before: LiveSourceProductState,
    after: LiveSourceProductState,
    next: map::IntegratedAdmissionState,
)
    requires
        live_source_product_wf(before),
        product_score_wf(after.product),
        product_map_step(
            before.product, after.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ),
        map::integrated_admission_driver_step(
            before.product.map,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ) == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted { state: next }),
        after.runtime == runtime_source::runtime_after_admission_step(
            before.runtime, after.product.bridge.admission,
        ),
        after.arena == before.arena,
        after.wire == before.wire.drop_last(),
    ensures live_source_product_wf(after),
{
    map::integrated_message_parent_accept_pop_shape(before.product.map, next);
    assert(after.product.map == next);
    assert(after.product.bridge.admission == next.admission);
    assert(after.product.bridge.transcript == next.transcript);
    assert(after.product.bridge.transcript == before.product.bridge.transcript);
    assert(after.runtime.score == after.product.bridge);
    assert(after.product.bridge.admission.frames.len() + 1
        == before.product.bridge.admission.frames.len());
    assert(admission::admission_state_wf(after.runtime.score.admission));
    assert(score_bridge::bridge_well_formed(after.runtime.score));
    assert(after.runtime.score.admission.frames.len() > 0);
    assert(after.runtime.score.admission.frames.len()
        <= before.runtime.score.admission.frames.len());
    assert(after.runtime.capacity == before.runtime.capacity);
    assert(after.runtime.max_frames == before.runtime.max_frames);
    assert(after.runtime.max_storage_bytes == before.runtime.max_storage_bytes);
    assert(after.runtime.frame_bytes == before.runtime.frame_bytes);
    assert(after.runtime.max_capacity_bytes == before.runtime.max_capacity_bytes);
    assert(after.runtime.frame_bytes > 0);
    assert(after.runtime.score.admission.frames.len() <= after.runtime.capacity as int);
    assert(after.runtime.capacity as int * after.runtime.frame_bytes as int
        <= usize::MAX as int);
    assert(after.runtime.max_frames as int
        >= after.runtime.score.admission.frames.len());
    let old_len = before.runtime.score.admission.frames.len();
    let new_len = after.runtime.score.admission.frames.len();
    let frame_bytes = after.runtime.frame_bytes as int;
    let max_storage_bytes = after.runtime.max_storage_bytes as int;
    assert(max_storage_bytes >= old_len * frame_bytes);
    assert(new_len <= old_len);
    assert(frame_bytes > 0);
    assert(max_storage_bytes >= new_len * frame_bytes)
        by (nonlinear_arith)
        requires
            max_storage_bytes >= old_len * frame_bytes,
            new_len <= old_len,
            frame_bytes > 0;
    assert(after.runtime.max_capacity_bytes as int
        >= after.runtime.capacity as int * after.runtime.frame_bytes as int);
    assert(runtime_source::runtime_frame_image_wf(after.runtime));
    assert(arena_source::runtime_projects_score(
        after.arena, after.product.bridge.transcript,
    ));
    assert(after.wire.len() == after.product.bridge.admission.frames.len());
    assert forall|i: int| 0 <= i < after.wire.len() implies {
        let frame = after.product.bridge.admission.frames[i];
        match #[trigger] after.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire_source::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    } by {
        assert(after.wire[i] == before.wire[i]);
        if i < after.wire.len() - 1 {
            assert(after.product.bridge.admission.frames[i]
                == before.product.bridge.admission.frames[i]);
        } else {
            assert(i == after.product.bridge.admission.frames.len() - 1);
            assert(i == before.product.bridge.admission.frames.len() - 2);
            assert(after.product.bridge.admission.frames[i].frame_kind
                == before.product.bridge.admission.frames[i].frame_kind);
            assert(after.product.bridge.admission.frames[i].region_start
                == before.product.bridge.admission.frames[i].region_start);
            assert(after.product.bridge.admission.frames[i].region_end
                == before.product.bridge.admission.frames[i].region_end);
            assert(after.product.bridge.admission.frames[i].cursor
                == before.product.bridge.admission.frames[i].cursor);
            assert(after.product.bridge.admission.frames[i].source_bytes
                == before.product.bridge.admission.frames[i].source_bytes);
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_child_complete_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_message_child_complete_step(before, after),
    ensures
        live_runtime_product_wf(after),
        after.source.product.bridge.admission.frames.len() + 1
            == before.source.product.bridge.admission.frames.len(),
        after.source.product.bridge.admission.completed as int
            == before.source.product.bridge.admission.completed as int + 1,
{
    let cursor = before.cursors.last().unwrap();
    wire_source::wire_runtime_end_is_exact(cursor);
    product_map_step_preserves_exact(
        before.source.product, after.source.product,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        },
    );
    live_runtime_wire_sources_pop(before, after);
    match map::integrated_admission_driver_step(
        before.source.product.map,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        },
    ) {
        Some(map::IntegratedAdmissionOutcome::MessageParentAccepted { state: next }) => {
            live_source_product_message_child_complete_preserves_wf(
                before.source, after.source, next,
            );
            map::integrated_message_parent_accept_pop_shape(before.source.product.map, next);
        },
        _ => assert(false),
    }
    assert(after.cursors.len() == after.source.wire.len());
    assert forall|i: int| 0 <= i < after.cursors.len() implies {
        match (#[trigger] after.cursors[i], after.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(after.cursors[i] == before.cursors[i]);
        assert(after.source.wire[i] == before.source.wire[i]);
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn product_epm_map_value_owned(state: ProductScoreState) -> bool {
    state.epm_frames.len() > 1
        && match state.epm_frames[state.epm_frames.len() - 2] {
            Some(parent) => match parent.wait {
                Some(epm::EpmWait::MapValue { .. }) => true,
                _ => false,
            },
            None => false,
        }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_epm_map_value_accept_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
) -> bool {
    if !live_runtime_product_wf(before) || before.cursors.len() <= 1
        || !product_epm_map_value_owned(before.source.product)
        || before.source.product.bridge.admission.frames.last().frame_kind != 1
    {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(cursor) => {
                &&& wire_source::wire_runtime_next_field(cursor)
                    == Some(wire_source::WireConcreteTransition::End {
                        state: wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                    })
                &&& product_epm_child_return_step(
                    before.source.product, after.source.product,
                    summary, admission_summary, None,
                )
                &&& after.source.runtime
                    == runtime_source::runtime_after_admission_step(
                        before.source.runtime,
                        after.source.product.bridge.admission,
                    )
                &&& after.source.arena == before.source.arena
                &&& after.source.wire == before.source.wire.drop_last()
                &&& after.cursors == before.cursors.drop_last()
            },
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_source_product_epm_map_value_accept_preserves_wf(
    before: LiveSourceProductState,
    after: LiveSourceProductState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
)
    requires
        live_source_product_wf(before),
        product_score_wf(after.product),
        product_epm_map_value_owned(before.product),
        product_epm_child_return_step(
            before.product, after.product, summary, admission_summary, None,
        ),
        after.runtime == runtime_source::runtime_after_admission_step(
            before.runtime, after.product.bridge.admission,
        ),
        after.arena == before.arena,
        after.wire == before.wire.drop_last(),
    ensures live_source_product_wf(after),
{
    product_epm_child_return_source_exact(
        before.product, after.product, summary, admission_summary, None,
    );
    let prior = product_linked_parent(before.product).unwrap();
    let next = product_linked_top(after.product).unwrap();
    epm_nested_source::source_epm_map_value_return_pop_shape(
        prior, next, before.product.epm_frames,
    );
    assert(after.product.bridge.admission == next.admission);
    assert(after.product.bridge.transcript == before.product.bridge.transcript);
    assert(after.runtime.score == after.product.bridge);
    assert(after.product.bridge.admission.frames.len() + 1
        == before.product.bridge.admission.frames.len());
    assert(admission::admission_state_wf(after.runtime.score.admission));
    assert(score_bridge::bridge_well_formed(after.runtime.score));
    assert(after.runtime.score.admission.frames.len() > 0);
    assert(after.runtime.score.admission.frames.len()
        <= before.runtime.score.admission.frames.len());
    assert(after.runtime.capacity == before.runtime.capacity);
    assert(after.runtime.max_frames == before.runtime.max_frames);
    assert(after.runtime.max_storage_bytes == before.runtime.max_storage_bytes);
    assert(after.runtime.frame_bytes == before.runtime.frame_bytes);
    assert(after.runtime.max_capacity_bytes == before.runtime.max_capacity_bytes);
    assert(after.runtime.frame_bytes > 0);
    assert(after.runtime.score.admission.frames.len() <= after.runtime.capacity as int);
    assert(after.runtime.capacity as int * after.runtime.frame_bytes as int
        <= usize::MAX as int);
    assert(after.runtime.max_frames as int
        >= after.runtime.score.admission.frames.len());
    let old_len = before.runtime.score.admission.frames.len();
    let new_len = after.runtime.score.admission.frames.len();
    let frame_bytes = after.runtime.frame_bytes as int;
    let max_storage_bytes = after.runtime.max_storage_bytes as int;
    assert(max_storage_bytes >= old_len * frame_bytes);
    assert(new_len <= old_len);
    assert(frame_bytes > 0);
    assert(max_storage_bytes >= new_len * frame_bytes)
        by (nonlinear_arith)
        requires
            max_storage_bytes >= old_len * frame_bytes,
            new_len <= old_len,
            frame_bytes > 0;
    assert(after.runtime.max_capacity_bytes as int
        >= after.runtime.capacity as int * after.runtime.frame_bytes as int);
    assert(runtime_source::runtime_frame_image_wf(after.runtime));
    assert(arena_source::runtime_projects_score(
        after.arena, after.product.bridge.transcript,
    ));
    assert(after.wire.len() == after.product.bridge.admission.frames.len());
    assert forall|i: int| 0 <= i < after.wire.len() implies {
        let frame = after.product.bridge.admission.frames[i];
        match #[trigger] after.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire_source::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    } by {
        assert(after.wire[i] == before.wire[i]);
        if i < after.wire.len() - 1 {
            assert(after.product.bridge.admission.frames[i]
                == before.product.bridge.admission.frames[i]);
        } else {
            assert(i == after.product.bridge.admission.frames.len() - 1);
            assert(i == before.product.bridge.admission.frames.len() - 2);
            assert(before.product.bridge.admission.frames[i].frame_kind == 3);
            assert(before.wire[i].is_none());
            assert(after.product.bridge.admission.frames[i].frame_kind == 3);
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_epm_map_value_accept_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
)
    requires live_runtime_product_epm_map_value_accept_step(
        before, after, summary, admission_summary,
    ),
    ensures
        live_runtime_product_wf(after),
        after.source.product.bridge.admission.frames.len() + 1
            == before.source.product.bridge.admission.frames.len(),
        after.source.product.bridge.admission.completed as int
            == before.source.product.bridge.admission.completed as int + 1,
{
    let cursor = before.cursors.last().unwrap();
    wire_source::wire_runtime_end_is_exact(cursor);
    product_epm_child_return_preserves_exact(
        before.source.product, after.source.product,
        summary, admission_summary, None,
    );
    live_source_product_epm_map_value_accept_preserves_wf(
        before.source, after.source, summary, admission_summary,
    );
    product_epm_child_return_source_exact(
        before.source.product, after.source.product,
        summary, admission_summary, None,
    );
    live_runtime_wire_sources_pop(before, after);
    let prior = product_linked_parent(before.source.product).unwrap();
    let next = product_linked_top(after.source.product).unwrap();
    epm_nested_source::source_epm_map_value_return_pop_shape(
        prior, next, before.source.product.epm_frames,
    );
    assert(after.cursors.len() == after.source.wire.len());
    assert forall|i: int| 0 <= i < after.cursors.len() implies {
        match (#[trigger] after.cursors[i], after.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire_source::wire_concrete_cursor_wf(
                    wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(after.cursors[i] == before.cursors[i]);
        assert(after.source.wire[i] == before.source.wire[i]);
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_message_push_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
) -> bool {
    if !live_runtime_product_wf(before) || before.cursors.len() == 0 {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(parent) => match wire_source::wire_runtime_next_field(parent) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field, start: _,
                }) => match live_runtime_child_cursor(parent, field) {
                    None => false,
                    Some(child) => {
                        let next_parent = runtime_wire_after_field(parent, concrete_after);
                        match (
                            wire_source::wire_runtime_cursor_project(next_parent),
                            wire_source::wire_runtime_cursor_project(child),
                        ) {
                            (Some(next_parent_reader), Some(child_reader)) => {
                                &&& request.field == field
                                &&& request.child.source_bytes == field.payload_bytes
                                &&& (request.child.frame_kind == 1
                                    || request.child.frame_kind == 2)
                                &&& map::integrated_source_message_action(
                                    before.source.product.map.admission,
                                    admission::ConcreteFrameActionModel::Push {
                                        field,
                                        child: request.child,
                                        frame_bytes: before.source.runtime.frame_bytes,
                                    },
                                ) == Some(admission::ConcreteFrameActionModel::Push {
                                    field,
                                    child: request.child,
                                    frame_bytes: before.source.runtime.frame_bytes,
                                })
                                &&& product_map_step(
                                    before.source.product, after.source.product,
                                    map::IntegratedAdmissionAction::Message {
                                        action: admission::ConcreteFrameActionModel::Push {
                                            field,
                                            child: request.child,
                                            frame_bytes: before.source.runtime.frame_bytes,
                                        },
                                    },
                                )
                                &&& runtime_source::source_checked_frame_request_push(
                                    before.source.runtime, request, storage,
                                ) == Some(after.source.runtime)
                                &&& after.source.arena == before.source.arena
                                &&& after.source.wire == before.source.wire.update(
                                    before.source.wire.len() - 1, Some(next_parent_reader),
                                ).push(Some(child_reader))
                                &&& after.cursors == before.cursors.update(
                                    before.cursors.len() - 1, Some(next_parent),
                                ).push(Some(child))
                            },
                            _ => false,
                        }
                    },
                },
                _ => false,
            },
        }
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_message_push_indexed_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
) -> bool {
    if !live_runtime_product_wf(before)
        || !product_frame_source_anchors_match(before.source.product)
        || before.cursors.len() == 0
        || before.source.product.indexed_regions.len() == 0
    {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(parent) => match wire_source::wire_runtime_next_field(parent) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field, start: _,
                }) => match live_runtime_child_cursor(parent, field) {
                    None => false,
                    Some(child) => {
                        let parent_source = product_admission_source_image(
                            before.source.product.indexed_regions.last(),
                        );
                        let next_parent = runtime_wire_after_field(parent, concrete_after);
                        match (
                            wire_source::wire_runtime_cursor_project(next_parent),
                            wire_source::wire_runtime_cursor_project(child),
                        ) {
                            (Some(next_parent_reader), Some(child_reader)) => {
                                &&& product_wire_indexed_source_agrees(
                                    parent, before.source.product.indexed_regions.last(),
                                )
                                &&& request.field == field
                                &&& request.child.source_bytes == field.payload_bytes
                                &&& (request.child.frame_kind == 1
                                    || request.child.frame_kind == 2)
                                &&& map::integrated_source_message_action(
                                    before.source.product.map.admission,
                                    admission::ConcreteFrameActionModel::PushIndexed {
                                        field,
                                        child: request.child,
                                        parent_source,
                                        frame_bytes: before.source.runtime.frame_bytes,
                                    },
                                ) == Some(admission::ConcreteFrameActionModel::PushIndexed {
                                    field,
                                    child: request.child,
                                    parent_source,
                                    frame_bytes: before.source.runtime.frame_bytes,
                                })
                                &&& product_map_step(
                                    before.source.product, after.source.product,
                                    map::IntegratedAdmissionAction::Message {
                                        action: admission::ConcreteFrameActionModel::PushIndexed {
                                            field,
                                            child: request.child,
                                            parent_source,
                                            frame_bytes: before.source.runtime.frame_bytes,
                                        },
                                    },
                                )
                                &&& runtime_source::source_checked_frame_request_push_indexed(
                                    before.source.runtime, request, parent_source, storage,
                                ) == Some(after.source.runtime)
                                &&& after.source.arena == before.source.arena
                                &&& after.source.wire == before.source.wire.update(
                                    before.source.wire.len() - 1, Some(next_parent_reader),
                                ).push(Some(child_reader))
                                &&& after.cursors == before.cursors.update(
                                    before.cursors.len() - 1, Some(next_parent),
                                ).push(Some(child))
                            },
                            _ => false,
                        }
                    },
                },
                _ => false,
            },
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_indexed_is_checked(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_message_push_indexed_step(
        before, after, request, storage,
    ),
    ensures
        runtime_source::source_frame_request_matches_child(request),
        runtime_source::source_successful_push_storage(before.source.runtime, storage),
        runtime_source::runtime_frame_image_wf(after.source.runtime),
        after.source.runtime.score.admission.frames.len() == storage.after_len as int,
        after.source.runtime.max_storage_bytes >= storage.used_bytes,
        after.source.runtime.max_capacity_bytes >= storage.reserved_bytes,
        after.source.wire.last().unwrap().cursor.source_bytes
            == request.child.source_bytes,
        after.source.product.indexed_regions.last().bytes
            == request.child.source_bytes,
        after.source.product.bridge.admission.frames.last().physical_start
            == product_indexed_start_anchor(
                after.source.product.indexed_regions.last(),
            ).offset,
        after.source.product.bridge.admission.frames.last().physical_source
            == product_indexed_start_anchor(
                after.source.product.indexed_regions.last(),
            ).source,
{
    let parent_source = product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    runtime_source::source_checked_frame_request_push_indexed_refines(
        before.source.runtime, request, parent_source, storage,
    );
    let parent = before.cursors.last().unwrap();
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(parent) {
        live_runtime_child_cursor_is_payload(parent, concrete_after, field, start);
        product_map_indexed_push_anchor_exact(
            before.source.product,
            after.source.product,
            field,
            request.child,
            parent_source,
            before.source.runtime.frame_bytes,
        );
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_indexed_source_ids_match(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_message_push_indexed_step(
        before, after, request, storage,
    ),
    ensures product_wire_indexed_source_agrees(
        after.cursors.last().unwrap(),
        after.source.product.indexed_regions.last(),
    ),
    after.source.product.bridge.admission.frames.last().physical_source
        == product_indexed_start_anchor(
            after.source.product.indexed_regions.last(),
        ).source,
{
    let parent = before.cursors.last().unwrap();
    let image = before.source.product.indexed_regions.last();
    if let Some(wire_source::WireConcreteTransition::Field {
        field, ..
    }) = wire_source::wire_runtime_next_field(parent) {
        assert(product_indexed_child(image, field).is_some());
        product_wire_indexed_child_source_ids_exact(parent, image, field);
        product_map_indexed_push_anchor_exact(
            before.source.product, after.source.product,
            field, request.child, product_admission_source_image(image),
            before.source.runtime.frame_bytes,
        );
        assert(after.cursors.last() == live_runtime_child_cursor(parent, field));
        assert(after.source.product.indexed_regions.last()
            == product_indexed_child(image, field).unwrap());
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_indexed_components_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_message_push_indexed_step(
        before, after, request, storage,
    ),
    ensures
        product_score_wf(after.source.product),
        runtime_source::runtime_frame_image_wf(after.source.runtime),
{
    let parent_source = product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    runtime_source::source_checked_frame_request_push_indexed_refines(
        before.source.runtime, request, parent_source, storage,
    );
    let parent = before.cursors.last().unwrap();
    if let Some(wire_source::WireConcreteTransition::Field {
        field, ..
    }) = wire_source::wire_runtime_next_field(parent) {
        product_map_step_preserves_exact(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::PushIndexed {
                    field,
                    child: request.child,
                    parent_source,
                    frame_bytes: before.source.runtime.frame_bytes,
                },
            },
        );
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_indexed_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_message_push_indexed_step(
        before, after, request, storage,
    ),
    ensures live_runtime_product_wf(after),
{
    live_runtime_product_message_push_indexed_is_checked(
        before, after, request, storage,
    );
    live_runtime_product_message_push_indexed_components_wf(
        before, after, request, storage,
    );
    let parent = before.cursors.last().unwrap();
    let parent_source = product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(parent) {
        live_runtime_child_cursor_is_payload(parent, concrete_after, field, start);
        live_runtime_parent_cursor_after_field_projects(
            parent, concrete_after, field, start,
        );
        live_runtime_wire_sources_push(before, after, field);
        assert(after.source.runtime.score.admission
            == after.source.product.bridge.admission);
        assert(after.source.runtime.score.transcript
            == after.source.product.bridge.transcript);
        assert(after.source.runtime.score == after.source.product.bridge);
        assert(after.source.product.bridge.transcript
            == before.source.product.bridge.transcript);
        assert(arena_source::runtime_projects_score(
            after.source.arena, after.source.product.bridge.transcript,
        ));
    }
    assert(live_runtime_product_wf(after));
}

// A PathMap field pushes an EPathMap frame, not a protobuf field reader. The
// indexed child still owns the exact payload view and its physical positions;
// the EPathMap topology cursor is initialized by the subsequent handoff.
#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_message_epm_push_indexed_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
) -> bool {
    if !live_runtime_product_wf(before) || before.cursors.len() == 0
        || before.source.product.indexed_regions.len() == 0
    {
        false
    } else {
        match before.cursors.last() {
            None => false,
            Some(parent) => match wire_source::wire_runtime_next_field(parent) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field, start: _,
                }) => {
                    let parent_source = product_admission_source_image(
                        before.source.product.indexed_regions.last(),
                    );
                    let next_parent = runtime_wire_after_field(parent, concrete_after);
                    match wire_source::wire_runtime_cursor_project(next_parent) {
                        None => false,
                        Some(next_parent_reader) => {
                            let action = admission::ConcreteFrameActionModel::PushIndexed {
                                field,
                                child: request.child,
                                parent_source,
                                frame_bytes: before.source.runtime.frame_bytes,
                            };
                            &&& request.request_kind
                                == runtime_source::SourceFrameRequestKind::EPathMap
                            &&& request.child.frame_kind == 3
                            &&& request.field == field
                            &&& request.child.source_bytes == field.payload_bytes
                            &&& product_wire_indexed_source_agrees(
                                parent, before.source.product.indexed_regions.last(),
                            )
                            &&& map::integrated_source_message_action(
                                before.source.product.map.admission, action,
                            ) == Some(action)
                            &&& product_map_step(
                                before.source.product, after.source.product,
                                map::IntegratedAdmissionAction::Message { action },
                            )
                            &&& runtime_source::source_checked_frame_request_push_indexed(
                                before.source.runtime, request, parent_source, storage,
                            ) == Some(after.source.runtime)
                            &&& after.source.arena == before.source.arena
                            &&& after.source.wire == before.source.wire.update(
                                before.source.wire.len() - 1, Some(next_parent_reader),
                            ).push(None)
                            &&& after.cursors == before.cursors.update(
                                before.cursors.len() - 1, Some(next_parent),
                            ).push(None)
                        },
                    }
                },
                _ => false,
            },
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_epm_push_indexed_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_message_epm_push_indexed_step(
        before, after, request, storage,
    ),
    ensures
        live_runtime_product_wf(after),
        runtime_source::source_frame_request_matches_child(request),
        runtime_source::source_successful_push_storage(before.source.runtime, storage),
        after.source.runtime.score.admission.frames.len() == storage.after_len as int,
        after.source.product.bridge.admission.frames.last().frame_kind == 3,
        after.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 1
        ].waiting_kind == 3,
        after.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 1
        ].waiting_tag == Some(request.field.number),
        after.source.product.bridge.admission.frames.last().physical_source
            == product_indexed_start_anchor(
                after.source.product.indexed_regions.last(),
            ).source,
        after.source.product.bridge.admission.frames.last().physical_start
            == product_indexed_start_anchor(
                after.source.product.indexed_regions.last(),
            ).offset,
        live_runtime_child_cursor(
            before.cursors.last().unwrap(), request.field,
        ).is_some(),
        product_wire_indexed_source_agrees(
            live_runtime_child_cursor(
                before.cursors.last().unwrap(), request.field,
            ).unwrap(),
            after.source.product.indexed_regions.last(),
        ),
{
    let parent = before.cursors.last().unwrap();
    let image = before.source.product.indexed_regions.last();
    let parent_source = product_admission_source_image(image);
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(parent) {
        let action = admission::ConcreteFrameActionModel::PushIndexed {
            field,
            child: request.child,
            parent_source,
            frame_bytes: before.source.runtime.frame_bytes,
        };
        runtime_source::source_checked_frame_request_push_indexed_refines(
            before.source.runtime, request, parent_source, storage,
        );
        product_map_step_preserves_exact(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::Message { action },
        );
        product_map_indexed_push_anchor_exact(
            before.source.product, after.source.product, field,
            request.child, parent_source, before.source.runtime.frame_bytes,
        );
        live_runtime_child_cursor_is_payload(
            parent, concrete_after, field, start,
        );
        product_wire_indexed_child_source_ids_exact(parent, image, field);
        live_runtime_parent_cursor_after_field_projects(
            parent, concrete_after, field, start,
        );
        let parent_index = before.cursors.len() - 1;
        assert(after.source.product.bridge.admission.frames[parent_index].waiting_kind == 3);
        assert(after.source.product.bridge.admission.frames[parent_index].waiting_tag
            == Some(field.number));
        assert(after.source.product.bridge.admission.frames.last().frame_kind == 3);
        assert(after.source.runtime.score == after.source.product.bridge);
        assert(after.source.product.bridge.transcript
            == before.source.product.bridge.transcript);
        assert(arena_source::runtime_projects_score(
            after.source.arena, after.source.product.bridge.transcript,
        ));
        assert forall|i: int| 0 <= i < after.source.wire.len() implies {
            let frame = after.source.product.bridge.admission.frames[i];
            match #[trigger] after.source.wire[i] {
                None => frame.frame_kind == 3 || frame.frame_kind == 4,
                Some(reader) => {
                    &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                    &&& wire_source::wire_cursor_shape(reader.cursor)
                    &&& reader.cursor.region_start == frame.region_start
                    &&& reader.cursor.region_end == frame.region_end
                    &&& reader.cursor.next == frame.cursor
                    &&& reader.cursor.source_bytes == frame.source_bytes
                    &&& reader.terminal_error.is_none()
                },
            }
        } by {
            if i < parent_index {
                assert(after.source.wire[i] == before.source.wire[i]);
                assert(after.source.product.bridge.admission.frames[i]
                    == before.source.product.bridge.admission.frames[i]);
            } else if i == parent_index {
                assert(after.source.wire[i] == Some(
                    wire_source::wire_runtime_cursor_project(
                        runtime_wire_after_field(parent, concrete_after),
                    ).unwrap(),
                ));
                assert(after.source.product.bridge.admission.frames[i].cursor
                    == field.encoded_end);
            } else {
                assert(i == parent_index + 1);
                assert(after.source.wire[i].is_none());
            }
        }
        assert(live_source_product_wf(after.source));
        assert forall|i: int| 0 <= i < after.cursors.len() implies
            match #[trigger] after.cursors[i] {
                None => true,
                Some(cursor) => product_wire_indexed_source_agrees(
                    cursor, after.source.product.indexed_regions[i],
                ),
            } by {
            if i < parent_index {
                assert(after.cursors[i] == before.cursors[i]);
                assert(after.source.product.indexed_regions[i]
                    == before.source.product.indexed_regions[i]);
            } else if i == parent_index {
                product_wire_indexed_source_agrees_same_backing(
                    parent, runtime_wire_after_field(parent, concrete_after), image,
                );
            } else {
                assert(i == parent_index + 1);
                assert(after.cursors[i].is_none());
            }
        }
        assert forall|i: int| 0 <= i < after.cursors.len() implies {
            match (#[trigger] after.cursors[i], after.source.wire[i]) {
                (None, None) => true,
                (Some(cursor), Some(reader)) => {
                    &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
                    &&& wire_source::wire_concrete_cursor_wf(
                        wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                    )
                    &&& wire_source::wire_runtime_cursor_project(cursor)
                        == Some(reader)
                },
                _ => false,
            }
        } by {
            if i < parent_index {
                assert(after.cursors[i] == before.cursors[i]);
                assert(after.source.wire[i] == before.source.wire[i]);
            } else if i == parent_index {
                assert(after.cursors[i]
                    == Some(runtime_wire_after_field(parent, concrete_after)));
                assert(after.source.wire[i]
                    == wire_source::wire_runtime_cursor_project(
                        runtime_wire_after_field(parent, concrete_after),
                    ));
            } else {
                assert(i == parent_index + 1);
                assert(after.cursors[i].is_none());
                assert(after.source.wire[i].is_none());
            }
        }
    }
    assert(live_runtime_product_wf(after));
}

// The scalar storage fields are the ones checked after a real Vec push. The
// ghost request and both product images must still be extracted from Rust;
// this relation does not assert that every observed EPathMap push has them.
#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_message_epm_push_indexed_observed_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
) -> bool {
    if !live_runtime_product_wf(before)
        || before.source.product.bridge.admission.frames.len() > usize::MAX as int
        || stats_frame_variant_bytes != before.source.runtime.frame_bytes
    {
        false
    } else {
        match runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            before.source.product.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
            runtime_source::SourceFramePushObservationResult::Success { storage } =>
                live_runtime_product_message_epm_push_indexed_step(
                    before, after, request, storage,
                ),
            _ => false,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_epm_push_indexed_observed_is_exact(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires live_runtime_product_message_epm_push_indexed_observed_step(
        before, after, request, actual_top_kind,
        after_len, after_capacity, stats_frame_variant_bytes,
    ),
    ensures
        live_runtime_product_wf(after),
        request.request_kind == runtime_source::SourceFrameRequestKind::EPathMap,
        actual_top_kind == Some(runtime_source::SourceFrameRequestKind::EPathMap),
        after.source.runtime.score.admission.frames.len() == after_len as int,
        after.source.runtime.capacity == after_capacity,
        stats_frame_variant_bytes == before.source.runtime.frame_bytes,
        after.source.runtime.frame_bytes == stats_frame_variant_bytes,
        after.source.product.bridge.admission.frames.last().frame_kind == 3,
        after.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 1
        ].waiting_kind == 3,
        after.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 1
        ].waiting_tag == Some(request.field.number),
        after.source.wire.last().is_none(),
        after.cursors.last().is_none(),
        after.source.product.bridge.admission.frames.last().physical_source
            == product_indexed_start_anchor(
                after.source.product.indexed_regions.last(),
            ).source,
        after.source.product.bridge.admission.frames.last().physical_start
            == product_indexed_start_anchor(
                after.source.product.indexed_regions.last(),
            ).offset,
{
    runtime_source::source_frame_push_observe_after_exact(
        request.request_kind, actual_top_kind,
        before.source.product.bridge.admission.frames.len() as usize,
        before.source.runtime.capacity, after_len, after_capacity,
        before.source.runtime.frame_bytes, stats_frame_variant_bytes,
    );
    match runtime_source::source_frame_push_observe_after(
        request.request_kind, actual_top_kind,
        before.source.product.bridge.admission.frames.len() as usize,
        before.source.runtime.capacity, after_len, after_capacity,
        before.source.runtime.frame_bytes, stats_frame_variant_bytes,
    ) {
        runtime_source::SourceFramePushObservationResult::Success { storage } => {
            live_runtime_product_message_epm_push_indexed_preserves_wf(
                before, after, request, storage,
            );
            assert(request.request_kind
                == runtime_source::SourceFrameRequestKind::EPathMap);
            match actual_top_kind {
                Some(runtime_source::SourceFrameRequestKind::EPathMap) => {},
                _ => assert(false),
            }
            assert(storage.after_len == after_len);
            assert(storage.after_capacity == after_capacity);
        },
        _ => assert(false),
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_message_push_indexed_observed_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
) -> bool {
    if !live_runtime_product_wf(before)
        || before.source.product.bridge.admission.frames.len() > usize::MAX as int
        || stats_frame_variant_bytes != before.source.runtime.frame_bytes
    {
        false
    } else {
        match runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            before.source.product.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
            runtime_source::SourceFramePushObservationResult::Success { storage } =>
                live_runtime_product_message_push_indexed_step(
                    before, after, request, storage,
                ),
            _ => false,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_indexed_observed_is_exact(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires live_runtime_product_message_push_indexed_observed_step(
        before, after, request, actual_top_kind,
        after_len, after_capacity, stats_frame_variant_bytes,
    ),
    ensures
        live_runtime_product_wf(after),
        after.source.product.bridge.admission.frames.len() == after_len as int,
        after.source.runtime.capacity == after_capacity,
{
    match runtime_source::source_frame_push_observe_after(
        request.request_kind, actual_top_kind,
        before.source.product.bridge.admission.frames.len() as usize,
        before.source.runtime.capacity, after_len, after_capacity,
        before.source.runtime.frame_bytes, stats_frame_variant_bytes,
    ) {
        runtime_source::SourceFramePushObservationResult::Success { storage } => {
            live_runtime_product_message_push_indexed_preserves_wf(
                before, after, request, storage,
            );
            live_runtime_product_message_push_indexed_is_checked(
                before, after, request, storage,
            );
        },
        _ => assert(false),
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_is_checked(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_message_push_step(before, after, request, storage),
    ensures
        runtime_source::source_frame_request_matches_child(request),
        runtime_source::source_successful_push_storage(before.source.runtime, storage),
        after.source.runtime.score.admission.frames.len() == storage.after_len as int,
        after.source.runtime.max_storage_bytes >= storage.used_bytes,
        after.source.runtime.max_capacity_bytes >= storage.reserved_bytes,
        after.source.wire.last().unwrap().cursor.source_bytes
            == request.child.source_bytes,
{
    runtime_source::source_checked_frame_request_push_refines(
        before.source.runtime, request, storage,
    );
    let parent = before.cursors.last().unwrap();
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(parent) {
        live_runtime_child_cursor_is_payload(parent, concrete_after, field, start);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_components_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_message_push_step(before, after, request, storage),
    ensures
        product_score_wf(after.source.product),
        runtime_source::runtime_frame_image_wf(after.source.runtime),
{
    runtime_source::source_checked_frame_request_push_refines(
        before.source.runtime, request, storage,
    );
    let parent = before.cursors.last().unwrap();
    if let Some(wire_source::WireConcreteTransition::Field {
        field, ..
    }) = wire_source::wire_runtime_next_field(parent) {
        product_map_step_preserves_exact(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Push {
                    field,
                    child: request.child,
                    frame_bytes: before.source.runtime.frame_bytes,
                },
            },
        );
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_score_agrees(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_message_push_step(before, after, request, storage),
    ensures after.source.runtime.score == after.source.product.bridge,
{
    live_runtime_product_message_push_components_wf(before, after, request, storage);
    let parent = before.cursors.last().unwrap();
    if let Some(wire_source::WireConcreteTransition::Field {
        field, ..
    }) = wire_source::wire_runtime_next_field(parent) {
        assert(request.field == field);
        assert(after.source.runtime.score.admission
            == after.source.product.bridge.admission);
        assert(after.source.runtime.score.transcript
            == after.source.product.bridge.transcript);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_arena_agrees(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_message_push_step(before, after, request, storage),
    ensures arena_source::runtime_projects_score(
        after.source.arena, after.source.product.bridge.transcript,
    ),
{
    live_runtime_product_message_push_score_agrees(before, after, request, storage);
    runtime_source::source_checked_frame_request_push_refines(
        before.source.runtime, request, storage,
    );
    assert(after.source.runtime.score.transcript
        == before.source.runtime.score.transcript);
    assert(after.source.product.bridge.transcript
        == before.source.product.bridge.transcript);
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires live_runtime_product_message_push_step(before, after, request, storage),
    ensures live_runtime_product_wf(after),
{
    live_runtime_product_message_push_is_checked(before, after, request, storage);
    live_runtime_product_message_push_components_wf(before, after, request, storage);
    live_runtime_product_message_push_score_agrees(before, after, request, storage);
    live_runtime_product_message_push_arena_agrees(before, after, request, storage);
    let parent = before.cursors.last().unwrap();
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(parent) {
        live_runtime_child_cursor_is_payload(parent, concrete_after, field, start);
        live_runtime_parent_cursor_after_field_projects(
            parent, concrete_after, field, start,
        );
        live_runtime_wire_sources_push(before, after, field);
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_message_push_observed_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
) -> bool {
    if !live_runtime_product_wf(before)
        || before.source.product.bridge.admission.frames.len() > usize::MAX as int
        || stats_frame_variant_bytes != before.source.runtime.frame_bytes
    {
        false
    } else {
        match runtime_source::source_frame_push_observe_after(
            request.request_kind, actual_top_kind,
            before.source.product.bridge.admission.frames.len() as usize,
            before.source.runtime.capacity, after_len, after_capacity,
            before.source.runtime.frame_bytes, stats_frame_variant_bytes,
        ) {
            runtime_source::SourceFramePushObservationResult::Success { storage } =>
                live_runtime_product_message_push_step(
                    before, after, request, storage,
                ) || live_runtime_product_message_push_indexed_step(
                    before, after, request, storage,
                ),
            _ => false,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_push_observed_is_exact(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires live_runtime_product_message_push_observed_step(
        before, after, request, actual_top_kind,
        after_len, after_capacity, stats_frame_variant_bytes,
    ),
    ensures
        live_runtime_product_wf(after),
        after.source.runtime.score.admission.frames.len() == after_len as int,
        after.source.runtime.capacity == after_capacity,
{
    if let runtime_source::SourceFramePushObservationResult::Success { storage } =
        runtime_source::source_frame_push_observe_after(
        request.request_kind, actual_top_kind,
        before.source.product.bridge.admission.frames.len() as usize,
        before.source.runtime.capacity, after_len, after_capacity,
        before.source.runtime.frame_bytes, stats_frame_variant_bytes,
    ) {
        runtime_source::source_frame_push_observed_storage_is_checked(
            before.source.runtime, request.request_kind,
            actual_top_kind, after_len, after_capacity,
        );
        if live_runtime_product_message_push_step(before, after, request, storage) {
            live_runtime_product_message_push_is_checked(before, after, request, storage);
            live_runtime_product_message_push_preserves_wf(before, after, request, storage);
        } else {
            live_runtime_product_message_push_indexed_is_checked(
                before, after, request, storage,
            );
            live_runtime_product_message_push_indexed_preserves_wf(
                before, after, request, storage,
            );
        }
    }
}

// A source-selected root score changes the score arena and the top frame but
// leaves the wire reader, indexed backing, and checked frame storage in place.
#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_root_score_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    &&& live_runtime_product_wf(before)
    &&& before.cursors.len() == 1
    &&& before.cursors[0].is_some()
    &&& wire_source::wire_runtime_next_field(before.cursors[0].unwrap())
        == Some(wire_source::WireConcreteTransition::End {
            state: wire_source::wire_runtime_derive_cursor(
                before.cursors[0].unwrap(),
            ).unwrap(),
        })
    &&& score_bridge::bridge_score_finish_selected(
        before.source.product.bridge,
    ) == Some(after.source.product.bridge)
    &&& product_root_score_step(before.source.product, after.source.product)
    &&& after.source.runtime == runtime_source::RuntimeFrameImage {
        score: after.source.product.bridge,
        ..before.source.runtime
    }
    &&& after.source.arena == product_runtime_arena_from_score(
        after.source.product.bridge.transcript,
    )
    &&& after.source.wire == before.source.wire
    &&& after.cursors == before.cursors
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_root_score_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_root_score_step(before, after),
    ensures live_runtime_product_wf(after),
{
    product_root_score_step_preserves_exact(
        before.source.product, after.source.product,
    );
    score_bridge::bridge_score_finish_selected_preserves_checked(
        before.source.product.bridge,
    );
    score_bridge::bridge_score_source_step_preserves_shape(
        before.source.product.bridge, after.source.product.bridge,
    );
    product_runtime_arena_from_score_projects(
        after.source.product.bridge.transcript,
    );
    assert(runtime_source::runtime_frame_image_wf(after.source.runtime));
    assert(live_source_product_wf(after.source));
    assert(live_runtime_product_wf(after));
}

// Production message EOF performs this score step before the child pop and
// parent acceptance.  This transition keeps the live child cursor available
// for the following Complete action.
#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_message_child_score_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    &&& live_runtime_product_wf(before)
    &&& before.cursors.len() > 1
    &&& before.cursors.last().is_some()
    &&& before.source.product.bridge.admission.frames.last().waiting_id.is_none()
    &&& before.source.product.bridge.admission.frames.last().score_handle.is_none()
    &&& before.source.product.bridge.admission.frames.last().cursor
        == before.source.product.bridge.admission.frames.last().region_end
    &&& runtime_source::runtime_noscore_child_kind(
        before.source.runtime,
    ).is_none()
    &&& wire_source::wire_runtime_next_field(before.cursors.last().unwrap())
        == Some(wire_source::WireConcreteTransition::End {
            state: wire_source::wire_runtime_derive_cursor(
                before.cursors.last().unwrap(),
            ).unwrap(),
        })
    &&& score_bridge::bridge_score_finish_selected(
        before.source.product.bridge,
    ) == Some(after.source.product.bridge)
    &&& product_message_child_score_step(
        before.source.product, after.source.product,
    )
    &&& after.source.runtime == runtime_source::RuntimeFrameImage {
        score: after.source.product.bridge,
        ..before.source.runtime
    }
    &&& after.source.arena == product_runtime_arena_from_score(
        after.source.product.bridge.transcript,
    )
    &&& after.source.wire == before.source.wire
    &&& after.cursors == before.cursors
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_child_score_enables_runtime_gate(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_message_child_score_step(before, after),
    ensures runtime_source::runtime_child_eof_gate(
        before.source.runtime, after.source.product.bridge,
    ),
{
    product_message_child_score_preserves_exact(
        before.source.product, after.source.product,
    );
    score_bridge::bridge_score_source_step_preserves_driver_counts(
        before.source.product.bridge, after.source.product.bridge,
    );
    runtime_source::runtime_score_source_edge_exact(
        before.source.product.bridge, after.source.product.bridge,
    );
    assert(before.source.runtime.score == before.source.product.bridge);
    assert(runtime_source::runtime_score_source_edge(
        before.source.runtime.score, after.source.product.bridge,
    ));
    assert(after.source.product.bridge.admission.frames.len()
        == before.source.runtime.score.admission.frames.len());
    runtime_source::runtime_child_eof_gate_intro(
        before.source.runtime, after.source.product.bridge,
    );
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_child_score_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_message_child_score_step(before, after),
    ensures live_runtime_product_wf(after),
{
    live_runtime_product_message_child_score_enables_runtime_gate(before, after);
    product_message_child_score_preserves_exact(
        before.source.product, after.source.product,
    );
    score_bridge::bridge_score_finish_selected_preserves_checked(
        before.source.product.bridge,
    );
    score_bridge::bridge_score_source_step_preserves_shape(
        before.source.product.bridge, after.source.product.bridge,
    );
    product_runtime_arena_from_score_projects(
        after.source.product.bridge.transcript,
    );
    assert(runtime_source::runtime_frame_image_wf(after.source.runtime));
    assert(live_source_product_wf(after.source));
    assert(live_runtime_product_wf(after));
}

// The production EOF driver scores and returns the root in one operation.
// `after` is its pre-pop ghost image; `returned` is the observable terminal
// outcome with no live frame remaining.
#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_root_complete_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    returned: runtime_source::RuntimeRootReturnImage,
) -> bool {
    &&& live_runtime_product_root_score_step(before, after)
    &&& before.source.product.bridge.admission.frames[0].frame_kind == 1
    &&& before.source.product.bridge.admission.frames[0].waiting_id.is_none()
    &&& before.source.product.bridge.admission.frames[0].score_handle.is_none()
    &&& runtime_source::runtime_message_driver_step(
        before.source.runtime, runtime_source::RuntimeMessageEvent::Eof,
    ) == Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned })
}

#[cfg(all(rpb3_live_push, rpb3_terminal))]
pub proof fn live_runtime_product_root_complete_is_exact(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    returned: runtime_source::RuntimeRootReturnImage,
    claimed: Seq<u8>,
)
    requires live_runtime_product_root_complete_step(before, after, returned),
    ensures
        live_runtime_product_wf(after),
        returned.remaining_frames == 0,
        returned.pre_pop == after.source.runtime,
        match map::integrated_admission_driver_step(
            after.source.product.map,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ) {
            Some(map::IntegratedAdmissionOutcome::RootFinished { root_summary }) => {
                let handle = root_summary.score_handle;
                &&& root_summary == returned.summary
                &&& returned.score_bytes == after.source.product.bridge.admission.arena[
                    handle as int
                ].transcript_bytes
                &&& score_bridge::bridge_root_first_difference(
                    after.source.product.bridge, claimed,
                ).is_none() == rpb3_score_transcript::claimed_score_matches(
                    after.source.product.bridge.transcript, handle, claimed,
                )
            },
            _ => false,
        },
{
    live_runtime_product_root_score_preserves_wf(before, after);
    assert(before.source.runtime.score == before.source.product.bridge);
    assert(score_bridge::bridge_score_finish_selected(before.source.runtime.score)
        == Some(after.source.product.bridge));
    live_runtime_root_complete_matches_source(
        before, after.source.product, returned, claimed,
    );
    assert(runtime_source::runtime_message_eof_root_return(
        before.source.runtime, after.source.product.bridge,
    ) == Some(returned));
    runtime_source::runtime_root_return_is_pre_pop_exact(
        before.source.runtime, after.source.product.bridge,
    );
}

// One transition vocabulary for the live wire, admission, storage, score, and
// nested-map product.  The component steps below remain the precise guards;
// this dispatcher makes their common inductive invariant explicit.
pub enum LiveRuntimeProductAction {
    Continue,
    ContinueIndexed,
    MapKey,
    RootScore,
    RootComplete { returned: runtime_source::RuntimeRootReturnImage },
    MessageChildScore,
    MessagePush {
        request: runtime_source::SourceFrameRequestImage,
        actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
        after_len: usize,
        after_capacity: usize,
        stats_frame_variant_bytes: usize,
    },
    // Message PathMap field Push (child kind 3). The observed relation still
    // requires a separate extraction of the real Rust request and frame views.
    MessageEpmPush {
        request: runtime_source::SourceFrameRequestImage,
        actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
        after_len: usize,
        after_capacity: usize,
        stats_frame_variant_bytes: usize,
    },
    MapValuePush {
        request: runtime_source::SourceFrameRequestImage,
        actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
        after_len: usize,
        after_capacity: usize,
        stats_frame_variant_bytes: usize,
    },
    MessageChildComplete,
    MapValueAccept,
    MapEntryFinish,
    EpmMapValueAccept {
        summary: epm::ChildSummary,
        admission_summary: admission::AdmissionReturnedKind,
    },
    EpmContinue { event: epm::EpmEvent },
    EpmPush {
        observed: rpb3_epm_push_source_action_refinement::EpmPushLiveObservation,
    },
    EpmEnd {
        observed: rpb3_epathmap_terminal_source_observation::EpmTerminalSourceObservation,
    },
    EpmFinishAccept,
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
) -> bool {
    match action {
        LiveRuntimeProductAction::Continue =>
            live_runtime_product_continue_step(before, after),
        LiveRuntimeProductAction::ContinueIndexed =>
            live_runtime_product_continue_indexed_step(before, after),
        LiveRuntimeProductAction::MapKey =>
            live_runtime_product_map_key_step(before, after),
        LiveRuntimeProductAction::RootScore =>
            live_runtime_product_root_score_step(before, after),
        LiveRuntimeProductAction::RootComplete { returned } =>
            live_runtime_product_root_complete_step(before, after, returned),
        LiveRuntimeProductAction::MessageChildScore =>
            live_runtime_product_message_child_score_step(before, after),
        LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_runtime_product_message_push_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MessageEpmPush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_runtime_product_message_epm_push_indexed_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_runtime_product_map_value_push_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MessageChildComplete =>
            live_runtime_product_message_child_complete_step(before, after),
        LiveRuntimeProductAction::MapValueAccept =>
            live_runtime_product_map_value_accept_step(before, after),
        LiveRuntimeProductAction::MapEntryFinish =>
            live_runtime_product_map_entry_finish_step(before, after),
        LiveRuntimeProductAction::EpmMapValueAccept {
            summary, admission_summary,
        } => live_runtime_product_epm_map_value_accept_step(
            before, after, summary, admission_summary,
        ),
        LiveRuntimeProductAction::EpmContinue { event } =>
            live_runtime_product_wf(before)
                && rpb3_epm_unwind_product_lift::live_runtime_internal_epm_unwind_step(
                    before, after, event,
                ),
        LiveRuntimeProductAction::EpmPush { observed } =>
            rpb3_epm_push_live_action_refinement::epm_push_live_after(
                before, observed,
            ) == Some(after),
        LiveRuntimeProductAction::EpmEnd { observed } =>
            live_runtime_product_epm_end_source_step(before, after, observed),
        LiveRuntimeProductAction::EpmFinishAccept =>
            live_runtime_product_epm_finish_accept_step(before, after),
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_step_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
)
    requires live_runtime_product_step(before, after, action),
    ensures live_runtime_product_wf(after),
{
    match action {
        LiveRuntimeProductAction::Continue =>
            live_runtime_product_continue_preserves_wf(before, after),
        LiveRuntimeProductAction::ContinueIndexed =>
            live_runtime_product_continue_indexed_preserves_wf(before, after),
        LiveRuntimeProductAction::MapKey =>
            live_runtime_product_map_key_preserves_wf(before, after),
        LiveRuntimeProductAction::RootScore =>
            live_runtime_product_root_score_preserves_wf(before, after),
        LiveRuntimeProductAction::RootComplete { returned } =>
            live_runtime_product_root_score_preserves_wf(before, after),
        LiveRuntimeProductAction::MessageChildScore =>
            live_runtime_product_message_child_score_preserves_wf(before, after),
        LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_runtime_product_message_push_observed_is_exact(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MessageEpmPush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_runtime_product_message_epm_push_indexed_observed_is_exact(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => live_runtime_product_map_value_push_observed_is_exact(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        LiveRuntimeProductAction::MessageChildComplete =>
            live_runtime_product_message_child_complete_preserves_wf(before, after),
        LiveRuntimeProductAction::MapValueAccept =>
            live_runtime_product_map_value_accept_preserves_wf(before, after),
        LiveRuntimeProductAction::MapEntryFinish =>
            live_runtime_product_map_entry_finish_preserves_wf(before, after),
        LiveRuntimeProductAction::EpmMapValueAccept {
            summary, admission_summary,
        } => live_runtime_product_epm_map_value_accept_preserves_wf(
            before, after, summary, admission_summary,
        ),
        LiveRuntimeProductAction::EpmContinue { event } =>
            rpb3_epm_unwind_product_lift::live_runtime_internal_epm_continue_preserves_wf(
                before, after, event,
            ),
        LiveRuntimeProductAction::EpmPush { observed } => {
            assert(rpb3_epm_push_live_action_refinement::epm_push_live_ready(
                before, observed,
            ));
            rpb3_epm_push_live_action_refinement::epm_push_live_action_is_checked(
                before, observed,
            );
        },
        LiveRuntimeProductAction::EpmEnd { observed } =>
            live_runtime_product_epm_end_source_preserves_wf(before, after, observed),
        LiveRuntimeProductAction::EpmFinishAccept =>
            live_runtime_product_epm_finish_accept_preserves_wf(before, after),
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_ancestor_limit(
    before: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
) -> int {
    let count = before.source.product.bridge.admission.frames.len();
    match action {
        LiveRuntimeProductAction::MessageChildComplete
        | LiveRuntimeProductAction::MapValueAccept
        | LiveRuntimeProductAction::MapEntryFinish
        | LiveRuntimeProductAction::EpmMapValueAccept { .. }
        | LiveRuntimeProductAction::EpmFinishAccept => count - 2,
        _ => count - 1,
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn product_ancestor_view_same(
    before: ProductScoreState,
    after: ProductScoreState,
    index: int,
) -> bool {
    &&& after.bridge.admission.frames[index] == before.bridge.admission.frames[index]
    &&& after.map.map_frames[index] == before.map.map_frames[index]
    &&& after.indexed_regions[index] == before.indexed_regions[index]
    &&& after.epm_frames[index] == before.epm_frames[index]
    &&& after.accepted_epm_sources[index] == before.accepted_epm_sources[index]
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_ancestor_view_same(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    index: int,
) -> bool {
    &&& product_ancestor_view_same(
        before.source.product, after.source.product, index,
    )
    &&& after.source.product.map.admission.frames[index]
        == before.source.product.map.admission.frames[index]
    &&& after.source.runtime.score.admission.frames[index]
        == before.source.runtime.score.admission.frames[index]
    &&& after.source.wire[index] == before.source.wire[index]
    &&& after.cursors[index] == before.cursors[index]
}

#[cfg(rpb3_live_push)]
pub open spec fn strict_ancestor_sequence_shape<T>(
    before: Seq<T>,
    after: Seq<T>,
) -> bool {
    &&& before.len() > 0
    &&& (
        after == before
        || after == before.drop_last()
        || (after.len() == before.len() + 1
            && after == before.push(after.last()))
        || (after.len() == before.len()
            && after == before.update(before.len() - 1, after.last()))
        || (after.len() == before.len() + 1
            && after == before.update(before.len() - 1, after[before.len() - 1])
                .push(after.last()))
    )
}

#[cfg(rpb3_live_push)]
pub proof fn strict_ancestor_sequence_shape_preserves_prefix<T>(
    before: Seq<T>,
    after: Seq<T>,
)
    requires strict_ancestor_sequence_shape(before, after),
    ensures forall|i: int| 0 <= i < before.len() - 1 ==> after[i] == before[i],
{
    if after == before {
    } else if after == before.drop_last() {
    } else if after.len() == before.len() + 1
        && after == before.push(after.last()) {
    } else if after.len() == before.len()
        && after == before.update(before.len() - 1, after.last()) {
    } else {
        assert(after == before.update(before.len() - 1, after[before.len() - 1])
            .push(after.last()));
    }
    assert forall|i: int| 0 <= i < before.len() - 1 implies
        after[i] == before[i] by {}
}

#[cfg(rpb3_live_push)]
pub proof fn live_wire_cursor_shape_preserves_prefix(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    limit: int,
)
    requires
        before.source.wire.len() == before.cursors.len(),
        0 <= limit <= before.cursors.len() - 1,
        strict_ancestor_sequence_shape(before.source.wire, after.source.wire),
        strict_ancestor_sequence_shape(before.cursors, after.cursors),
    ensures forall|i: int| 0 <= i < limit ==> {
        &&& after.source.wire[i] == before.source.wire[i]
        &&& after.cursors[i] == before.cursors[i]
    },
{
    strict_ancestor_sequence_shape_preserves_prefix(
        before.source.wire, after.source.wire,
    );
    strict_ancestor_sequence_shape_preserves_prefix(
        before.cursors, after.cursors,
    );
}

#[cfg(rpb3_live_push)]
pub proof fn product_carried_receipts_preserve_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
    limit: int,
)
    requires
        0 <= limit <= before.bridge.admission.frames.len(),
        limit <= after.bridge.admission.frames.len(),
        limit <= before.indexed_regions.len(),
        limit <= after.indexed_regions.len(),
        before.accepted_epm_sources.len() == before.bridge.admission.frames.len(),
        after.accepted_epm_sources == product_epm_receipts_carried(before, after),
        forall|i: int| 0 <= i < limit ==> {
            &&& #[trigger] before.bridge.admission.frames[i]
                == after.bridge.admission.frames[i]
            &&& before.indexed_regions[i] == after.indexed_regions[i]
        },
    ensures forall|i: int| 0 <= i < limit ==>
        after.accepted_epm_sources[i] == before.accepted_epm_sources[i],
{
    assert forall|i: int| 0 <= i < limit implies
        after.accepted_epm_sources[i] == before.accepted_epm_sources[i] by {
        assert(before.bridge.admission.frames[i] == after.bridge.admission.frames[i]);
        assert(before.indexed_regions[i] == after.indexed_regions[i]);
        assert(after.accepted_epm_sources[i]
            == product_epm_receipts_carried(before, after)[i]);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn product_message_child_score_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
)
    requires
        product_score_wf(before),
        product_message_child_score_step(before, after),
    ensures
        after.bridge.admission.frames.len() == before.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 1 ==>
            product_ancestor_view_same(before, after, i),
{
    score_bridge::bridge_score_source_step_preserves_shape(before.bridge, after.bridge);
    let limit = before.bridge.admission.frames.len() - 1;
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {}
    product_carried_receipts_preserve_prefix(before, after, limit);
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(before, after, i) by {}
}

#[cfg(rpb3_live_push)]
pub proof fn product_epm_continue_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
    event: epm::EpmEvent,
)
    requires
        product_score_wf(before),
        product_epm_continue_step(before, after, event),
    ensures
        after.bridge.admission.frames.len() == before.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 1 ==>
            product_ancestor_view_same(before, after, i),
{
    let prior = product_linked_top(before).unwrap();
    let next = product_linked_top(after).unwrap();
    let limit = before.bridge.admission.frames.len() - 1;
    assert(epm::linked_epm_continue(prior, next, event));
    assert(prior.admission == before.bridge.admission);
    assert(after.bridge.admission == next.admission);
    assert(next.admission.frames == prior.admission.frames.update(
        prior.admission.frames.len() - 1,
        epm::epm_advanced_admission_cursor(
            prior.admission.frames.last(), next.epm.cursor.value_table_offset,
        ),
    ));
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {
        assert(after.bridge.admission.frames[i] == before.bridge.admission.frames[i]);
    }
    product_carried_receipts_preserve_prefix(before, after, limit);
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(before, after, i) by {
        assert(after.epm_frames[i] == before.epm_frames[i]);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn product_epm_complete_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
    action: epm::EpmAction,
)
    requires
        product_score_wf(before),
        product_epm_complete_step(before, after, action),
    ensures
        after.bridge.admission.frames.len() == before.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 1 ==>
            product_ancestor_view_same(before, after, i),
{
    let limit = before.bridge.admission.frames.len() - 1;
    assert(score_bridge::bridge_same(before.bridge, after.bridge));
    assert(after.map == before.map);
    assert(after.indexed_regions == before.indexed_regions);
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {}
    product_carried_receipts_preserve_prefix(before, after, limit);
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(before, after, i) by {
        assert(after.epm_frames[i] == before.epm_frames[i]);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn product_epm_child_return_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
)
    requires
        product_score_wf(before),
        product_epm_child_return_step(
            before, after, summary, admission_summary, None,
        ),
    ensures
        after.bridge.admission.frames.len() + 1
            == before.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 2 ==>
            product_ancestor_view_same(before, after, i),
{
    let prior = product_linked_parent(before).unwrap();
    let next = product_linked_top(after).unwrap();
    let limit = before.bridge.admission.frames.len() - 2;
    assert(epm::linked_epm_child_return(
        prior, next, summary, admission_summary,
        before.epm_frames.last(), None,
    ));
    assert(prior.admission == before.bridge.admission);
    assert(after.bridge.admission == next.admission);
    assert(next.admission.frames == prior.admission.frames.subrange(
        0, prior.admission.frames.len() - 1,
    ).update(
        prior.admission.frames.len() - 2,
        epm::epm_parent_resolved(
            prior.admission.frames[prior.admission.frames.len() - 2],
        ),
    ));
    assert(after.indexed_regions == before.indexed_regions.drop_last());
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {
        assert(after.bridge.admission.frames[i] == before.bridge.admission.frames[i]);
    }
    product_carried_receipts_preserve_prefix(before, after, limit);
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(before, after, i) by {
        assert(after.map.map_frames[i] == before.map.map_frames[i]);
        assert(after.epm_frames[i] == before.epm_frames[i]);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn product_map_key_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
    field: admission::IndexedCanonicalFieldModel,
)
    requires
        product_score_wf(before),
        product_map_step(
            before, after, map::IntegratedAdmissionAction::MapKey { field },
        ),
    ensures
        after.bridge.admission.frames.len() == before.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 1 ==>
            product_ancestor_view_same(before, after, i),
{
    let count = before.bridge.admission.frames.len();
    let current = before.map.map_frames[count - 1].unwrap();
    let (next_frame, next_admission) = map::map_admission_key_continue(
        current, before.map.admission, field,
    ).unwrap();
    assert(after.bridge.admission == next_admission);
    assert(after.map.map_frames == before.map.map_frames.update(
        count - 1, Some(next_frame),
    ));
    assert(admission::admission_continue_step(
        before.map.admission, field.encoded_end, 0, false, 1,
    ) == Some(next_admission));
    assert(after.indexed_regions == before.indexed_regions);
    assert(after.epm_frames == before.epm_frames);
    let limit = count - 1;
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {
        assert(next_admission.frames[i] == before.map.admission.frames[i]);
    }
    product_carried_receipts_preserve_prefix(before, after, limit);
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(before, after, i) by {}
}

#[cfg(rpb3_live_push)]
pub proof fn live_epm_push_preserves_ancestor_prefix(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    observed: rpb3_epm_push_source_action_refinement::EpmPushLiveObservation,
)
    requires rpb3_epm_push_live_action_refinement::epm_push_live_after(
        before, observed,
    ) == Some(after),
    ensures
        live_runtime_product_wf(after),
        after.source.product.bridge.admission.frames.len()
            == before.source.product.bridge.admission.frames.len() + 1,
        forall|i: int| 0 <= i < before.source.product.bridge.admission.frames.len() - 1 ==>
            live_runtime_product_ancestor_view_same(before, after, i),
{
    assert(rpb3_epm_push_live_action_refinement::epm_push_live_ready(
        before, observed,
    ));
    rpb3_epm_push_live_action_refinement::epm_push_live_action_is_checked(
        before, observed,
    );
    let old = before.source.product;
    let new = after.source.product;
    let prior = product_linked_top(old).unwrap();
    let request = rpb3_epm_push_source_action_refinement::epm_push_source_request(
        prior, observed.push,
    ).unwrap();
    let next = rpb3_epm_push_source_action_refinement::epm_push_source_linked_after(
        prior, observed.push, request,
    );
    let limit = old.bridge.admission.frames.len() - 1;
    assert(new == rpb3_epm_push_source_action_refinement::epm_push_source_product_after(
        old, observed.push, request,
    ));
    assert(new.bridge.admission == next.admission);
    assert(prior.admission == old.bridge.admission);
    assert(after.source.wire == before.source.wire.push(
        rpb3_epm_push_live_action_refinement::epm_push_live_child_reader(observed),
    ));
    assert(after.cursors == before.cursors.push(observed.child_cursor));
    assert forall|i: int| 0 <= i < limit implies {
        &&& old.bridge.admission.frames[i] == new.bridge.admission.frames[i]
        &&& old.indexed_regions[i] == new.indexed_regions[i]
    } by {
        assert(new.bridge.admission.frames[i] == old.bridge.admission.frames[i]);
    }
    product_carried_receipts_preserve_prefix(old, new, limit);
    assert forall|i: int| 0 <= i < limit implies
        live_runtime_product_ancestor_view_same(before, after, i) by {
        assert(new.map.map_frames[i] == old.map.map_frames[i]);
        assert(new.epm_frames[i] == old.epm_frames[i]);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn product_epm_finish_accept_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
)
    requires
        product_score_wf(before),
        product_epm_finish_accept_step(before, after),
    ensures
        after.bridge.admission.frames.len() + 1
            == before.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 2 ==>
            product_ancestor_view_same(before, after, i),
{
    let prior = product_linked_top(before).unwrap();
    let accepted = epm_source_accept::epm_source_finish_accept(prior).unwrap();
    let child = prior.admission.frames.last();
    let base = admission::admission_complete_child_step(
        prior.admission, child.return_id.unwrap(), 3,
        admission::AdmissionReturnedKind::Other,
    ).unwrap();
    let limit = before.bridge.admission.frames.len() - 2;
    assert(after.bridge.admission == accepted);
    assert(prior.admission == before.bridge.admission);
    assert(after.indexed_regions == before.indexed_regions.drop_last());
    assert(after.map.map_frames == before.map.map_frames.drop_last());
    assert(after.epm_frames == before.epm_frames.drop_last());
    assert(base.frames == prior.admission.frames.subrange(
        0, prior.admission.frames.len() - 1,
    ).update(prior.admission.frames.len() - 2, base.frames.last()));
    assert(accepted.frames == base.frames.update(
        base.frames.len() - 1, accepted.frames.last(),
    ));
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {
        assert(after.bridge.admission.frames[i] == before.bridge.admission.frames[i]);
    }
    let carried = product_epm_receipts_carried(before, after);
    let carried_state = ProductScoreState {
        accepted_epm_sources: carried,
        ..after
    };
    product_carried_receipts_preserve_prefix(before, carried_state, limit);
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(before, after, i) by {
        assert(after.accepted_epm_sources[i] == carried[i]);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn concrete_message_continue_preserves_ancestor_prefix(
    before: admission::AdmissionState,
    after: admission::AdmissionState,
    action: admission::ConcreteFrameActionModel,
)
    requires
        before.frames.len() > 0,
        match action {
            admission::ConcreteFrameActionModel::Continue { .. }
            | admission::ConcreteFrameActionModel::ContinueIndexed { .. } => true,
            _ => false,
        },
        admission::concrete_message_driver_step(before, action)
            == Some(admission::ConcreteFrameOutcomeModel::Continued { state: after }),
    ensures
        after.frames.len() == before.frames.len(),
        forall|i: int| 0 <= i < before.frames.len() - 1 ==>
            after.frames[i] == before.frames[i],
{
    match action {
        admission::ConcreteFrameActionModel::Continue { field } => {
            let view = admission::concrete_message_view(before).unwrap();
            let next = admission::concrete_message_continue(view, field).unwrap();
            assert(after == next.projection);
            assert forall|i: int| 0 <= i < before.frames.len() - 1 implies
                after.frames[i] == before.frames[i] by {}
        },
        admission::ConcreteFrameActionModel::ContinueIndexed { field, parent_source } => {
            let view = admission::concrete_message_view(before).unwrap();
            let next = admission::concrete_message_continue_indexed(
                view, field, parent_source,
            ).unwrap();
            assert(after == next.projection);
            assert forall|i: int| 0 <= i < before.frames.len() - 1 implies
                after.frames[i] == before.frames[i] by {}
        },
        _ => assert(false),
    }
}

#[cfg(rpb3_live_push)]
pub proof fn product_message_continue_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
    action: admission::ConcreteFrameActionModel,
)
    requires
        product_score_wf(before),
        match action {
            admission::ConcreteFrameActionModel::Continue { .. }
            | admission::ConcreteFrameActionModel::ContinueIndexed { .. } => true,
            _ => false,
        },
        product_map_step(
            before, after, map::IntegratedAdmissionAction::Message { action },
        ),
    ensures
        after.bridge.admission.frames.len() == before.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 1 ==>
            product_ancestor_view_same(before, after, i),
{
    let next = after.map.admission;
    assert(admission::concrete_message_driver_step(before.map.admission, action)
        == Some(admission::ConcreteFrameOutcomeModel::Continued { state: next }));
    concrete_message_continue_preserves_ancestor_prefix(
        before.map.admission, next, action,
    );
    assert(after.bridge.admission == next);
    assert(after.map.map_frames == before.map.map_frames);
    assert(after.indexed_regions == before.indexed_regions);
    assert(after.epm_frames == before.epm_frames);
    let limit = before.bridge.admission.frames.len() - 1;
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {}
    product_carried_receipts_preserve_prefix(before, after, limit);
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(before, after, i) by {}
}

#[cfg(rpb3_live_push)]
pub proof fn admission_push_preserves_ancestor_prefix(
    before: admission::AdmissionState,
    after: admission::AdmissionState,
    parent_next: usize,
    child: admission::AdmissionFrameState,
    slot: usize,
    repeated: bool,
    field_tag: u32,
    frame_bytes: usize,
)
    requires
        before.frames.len() > 0,
        admission::admission_push_step(
            before, parent_next, child, slot, repeated, field_tag, frame_bytes,
        ) == Some(after),
    ensures
        after.frames.len() == before.frames.len() + 1,
        forall|i: int| 0 <= i < before.frames.len() - 1 ==>
            after.frames[i] == before.frames[i],
{
    assert(after.frames == before.frames.update(
        before.frames.len() - 1, after.frames[before.frames.len() - 1],
    ).push(after.frames.last()));
    assert forall|i: int| 0 <= i < before.frames.len() - 1 implies
        after.frames[i] == before.frames[i] by {}
}

#[cfg(rpb3_live_push)]
pub proof fn concrete_message_push_preserves_ancestor_prefix(
    before: admission::AdmissionState,
    after: admission::AdmissionState,
    action: admission::ConcreteFrameActionModel,
)
    requires
        before.frames.len() > 0,
        match action {
            admission::ConcreteFrameActionModel::Push { .. }
            | admission::ConcreteFrameActionModel::PushIndexed { .. } => true,
            _ => false,
        },
        admission::concrete_message_driver_step(before, action)
            == Some(admission::ConcreteFrameOutcomeModel::Pushed { state: after }),
    ensures
        after.frames.len() == before.frames.len() + 1,
        forall|i: int| 0 <= i < before.frames.len() - 1 ==>
            after.frames[i] == before.frames[i],
{
    match action {
        admission::ConcreteFrameActionModel::Push {
            field, child, frame_bytes,
        } => {
            let view = admission::concrete_message_view(before).unwrap();
            assert(admission::concrete_message_push(
                view, field, child, frame_bytes,
            ) == Some(after));
        },
        admission::ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source, frame_bytes,
        } => {
            let view = admission::concrete_message_view(before).unwrap();
            assert(admission::concrete_message_push_indexed(
                view, field, child, parent_source, frame_bytes,
            ) == Some(after));
        },
        _ => assert(false),
    }
    assert forall|i: int| 0 <= i < before.frames.len() - 1 implies
        after.frames[i] == before.frames[i] by {}
}

#[cfg(rpb3_live_push)]
pub proof fn product_message_push_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
    action: admission::ConcreteFrameActionModel,
)
    requires
        product_score_wf(before),
        match action {
            admission::ConcreteFrameActionModel::Push { .. }
            | admission::ConcreteFrameActionModel::PushIndexed { .. } => true,
            _ => false,
        },
        product_map_step(
            before, after, map::IntegratedAdmissionAction::Message { action },
        ),
    ensures
        after.bridge.admission.frames.len() == before.bridge.admission.frames.len() + 1,
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 1 ==>
            product_ancestor_view_same(before, after, i),
{
    let next = after.map.admission;
    let source_action = map::integrated_source_message_action(
        before.map.admission, action,
    ).unwrap();
    assert(map::integrated_admission_driver_step(
        before.map, map::IntegratedAdmissionAction::Message { action },
    ) == Some(map::IntegratedAdmissionOutcome::MessagePushed { state: after.map }));
    assert(admission::concrete_message_driver_step(before.map.admission, source_action)
        == Some(admission::ConcreteFrameOutcomeModel::Pushed { state: next }));
    match source_action {
        admission::ConcreteFrameActionModel::Push { .. }
        | admission::ConcreteFrameActionModel::PushIndexed { .. } =>
            concrete_message_push_preserves_ancestor_prefix(
                before.map.admission, next, source_action,
            ),
        _ => assert(false),
    }
    assert(after.bridge.admission == next);
    assert(after.map.map_frames.len() == before.map.map_frames.len() + 1);
    match action {
        admission::ConcreteFrameActionModel::Push { field, .. }
        | admission::ConcreteFrameActionModel::PushIndexed { field, .. } => {
            assert(after.indexed_regions == before.indexed_regions.push(
                product_indexed_child(before.indexed_regions.last(), field).unwrap(),
            ));
        },
        _ => assert(false),
    }
    assert(after.epm_frames == before.epm_frames.push(None));
    let limit = before.bridge.admission.frames.len() - 1;
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {}
    product_carried_receipts_preserve_prefix(before, after, limit);
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(before, after, i) by {}
}

#[cfg(rpb3_live_push)]
pub proof fn product_map_value_push_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
)
    requires
        product_score_wf(before),
        match action {
            map::IntegratedAdmissionAction::MapValue { .. }
            | map::IntegratedAdmissionAction::MapValueIndexed { .. } => true,
            _ => false,
        },
        product_map_step(before, after, action),
    ensures
        after.bridge.admission.frames.len() == before.bridge.admission.frames.len() + 1,
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 1 ==>
            product_ancestor_view_same(before, after, i),
{
    let count = before.bridge.admission.frames.len();
    let current = before.map.map_frames[count - 1].unwrap();
    match action {
        map::IntegratedAdmissionAction::MapValue {
            field, child, frame_bytes,
        } => {
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MapValuePushed {
                    state: after.map,
                }));
            let (next_frame, next_admission) = map::map_admission_value_push(
                current, before.map.admission, field, child, frame_bytes,
            ).unwrap();
            assert(after.bridge.admission == next_admission);
            assert(after.map.map_frames == before.map.map_frames.update(
                count - 1, Some(next_frame),
            ).push(None));
            assert(admission::admission_push_step(
                before.map.admission, field.encoded_end, child,
                1, false, 2, frame_bytes,
            ) == Some(next_admission));
            admission_push_preserves_ancestor_prefix(
                before.map.admission, next_admission,
                field.encoded_end, child, 1, false, 2, frame_bytes,
            );
            assert(after.indexed_regions == before.indexed_regions.push(
                product_indexed_child(before.indexed_regions.last(), field).unwrap(),
            ));
        },
        map::IntegratedAdmissionAction::MapValueIndexed {
            field, child, parent_source, frame_bytes,
        } => {
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MapValuePushed {
                    state: after.map,
                }));
            let (next_frame, next_admission) = map::map_admission_value_push_indexed(
                current, before.map.admission, field, child, parent_source, frame_bytes,
            ).unwrap();
            assert(after.bridge.admission == next_admission);
            assert(after.map.map_frames == before.map.map_frames.update(
                count - 1, Some(next_frame),
            ).push(None));
            assert(admission::admission_push_step(
                before.map.admission, field.encoded_end, child,
                1, false, 2, frame_bytes,
            ) == Some(next_admission));
            admission_push_preserves_ancestor_prefix(
                before.map.admission, next_admission,
                field.encoded_end, child, 1, false, 2, frame_bytes,
            );
            assert(after.indexed_regions == before.indexed_regions.push(
                product_indexed_child(before.indexed_regions.last(), field).unwrap(),
            ));
        },
        _ => assert(false),
    }
    assert(after.indexed_regions.len() == before.indexed_regions.len() + 1);
    assert(after.epm_frames == before.epm_frames.push(None));
    let limit = count - 1;
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {}
    product_carried_receipts_preserve_prefix(before, after, limit);
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(before, after, i) by {}
}

#[cfg(rpb3_live_push)]
pub proof fn admission_child_pop_preserves_ancestor_prefix(
    before: admission::AdmissionState,
    after: admission::AdmissionState,
    returned_id: u64,
    returned_kind: u8,
    summary: admission::AdmissionReturnedKind,
)
    requires admission::admission_complete_child_step(
        before, returned_id, returned_kind, summary,
    ) == Some(after),
    ensures
        after.frames.len() + 1 == before.frames.len(),
        forall|i: int| 0 <= i < before.frames.len() - 2 ==>
            after.frames[i] == before.frames[i],
{
    assert(after.frames == before.frames.subrange(0, before.frames.len() - 1)
        .update(before.frames.len() - 2, after.frames.last()));
    assert forall|i: int| 0 <= i < before.frames.len() - 2 implies
        after.frames[i] == before.frames[i] by {}
}

#[cfg(rpb3_live_push)]
pub proof fn product_map_pop_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
)
    requires
        product_score_wf(before),
        match action {
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            }
            | map::IntegratedAdmissionAction::MapAccept
            | map::IntegratedAdmissionAction::MapFinish => true,
            _ => false,
        },
        product_map_step(before, after, action),
        before.bridge.admission.frames.len() > 1,
    ensures
        after.bridge.admission.frames.len() + 1 == before.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 2 ==>
            product_ancestor_view_same(before, after, i),
{
    let count = before.bridge.admission.frames.len();
    let next = after.map.admission;
    match action {
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        } => {
            assert(map::integrated_source_message_action(
                before.map.admission,
                admission::ConcreteFrameActionModel::Complete,
            ) == Some(admission::ConcreteFrameActionModel::Complete));
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
                    state: after.map,
                }));
            admission::concrete_message_complete_child_simulates(before.map.admission);
            let child = before.map.admission.frames.last();
            let parent = before.map.admission.frames[count - 2];
            let summary = admission::concrete_derived_summary_with_arena(
                parent, child, before.map.admission.arena,
            ).unwrap();
            assert(admission::admission_complete_child_step(
                before.map.admission, child.return_id.unwrap(), child.frame_kind, summary,
            ) == Some(next));
            admission_child_pop_preserves_ancestor_prefix(
                before.map.admission, next,
                child.return_id.unwrap(), child.frame_kind, summary,
            );
            assert(after.map.map_frames == before.map.map_frames.drop_last());
        },
        map::IntegratedAdmissionAction::MapAccept => {
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MapValueAccepted {
                    state: after.map,
                }));
            let parent_frame = before.map.map_frames[count - 2].unwrap();
            let (next_frame, next_admission) = map::map_admission_value_complete(
                parent_frame, before.map.admission,
            ).unwrap();
            let wait = parent_frame.pending.unwrap();
            let summary = admission::AdmissionReturnedKind::Par {
                semantic_default: false,
                canonical_path_stable: next_frame.value.unwrap().canonical_path_stable,
            };
            assert(next == next_admission);
            assert(admission::admission_complete_child_step(
                before.map.admission, wait.id, 1, summary,
            ) == Some(next));
            admission_child_pop_preserves_ancestor_prefix(
                before.map.admission, next, wait.id, 1, summary,
            );
            assert(after.map.map_frames == before.map.map_frames.update(
                count - 2, Some(next_frame),
            ).subrange(0, count - 1));
        },
        map::IntegratedAdmissionAction::MapFinish => {
            let (summary, next_admission) = map::map_admission_entry_complete(
                before.map.map_frames[count - 1].unwrap(), before.map.admission,
            ).unwrap();
            assert(map::integrated_admission_driver_step(before.map, action)
                == Some(map::IntegratedAdmissionOutcome::MapEntryCompleted {
                    summary,
                    state: after.map,
                }));
            let map_frame = before.map.map_frames[count - 1].unwrap();
            assert(map::map_admission_entry_complete(map_frame, before.map.admission)
                == Some((summary, next_admission)));
            let child = before.map.admission.frames.last();
            let returned = admission::AdmissionReturnedKind::Map {
                key_bytes: summary.key_bytes,
                value_score_handle: admission::admission_map_value_score_handle(child),
            };
            assert(next == next_admission);
            assert(admission::admission_complete_child_step(
                before.map.admission, child.return_id.unwrap(), 2, returned,
            ) == Some(next));
            admission_child_pop_preserves_ancestor_prefix(
                before.map.admission, next, child.return_id.unwrap(), 2, returned,
            );
            assert(after.map.map_frames == before.map.map_frames.drop_last());
        },
        _ => assert(false),
    }
    assert(after.bridge.admission == next);
    assert(after.indexed_regions == before.indexed_regions.drop_last());
    assert(after.epm_frames == before.epm_frames.drop_last());
    let limit = count - 2;
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {}
    product_carried_receipts_preserve_prefix(before, after, limit);
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(before, after, i) by {}
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_step_preserves_ancestor_prefix(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
)
    requires live_runtime_product_step(before, after, action),
    ensures
        live_runtime_product_wf(after),
        0 <= live_runtime_product_ancestor_limit(before, action),
        live_runtime_product_ancestor_limit(before, action)
            <= after.source.product.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < live_runtime_product_ancestor_limit(before, action)
            ==> live_runtime_product_ancestor_view_same(before, after, i),
{
    live_runtime_product_step_preserves_wf(before, after, action);
    let old = before.source.product;
    let new = after.source.product;
    let limit = live_runtime_product_ancestor_limit(before, action);
    match action {
        LiveRuntimeProductAction::Continue => {
            assert(live_runtime_product_continue_step(before, after));
            match before.source.wire.last() {
                Some(reader) => match wire_source::wire_source_next_field(reader) {
                    wire_source::WireSourceTransition::Field { field, .. } =>
                        product_message_continue_preserves_ancestor_prefix(
                            old, new, admission::ConcreteFrameActionModel::Continue { field },
                        ),
                    _ => assert(false),
                },
                _ => assert(false),
            }
        },
        LiveRuntimeProductAction::ContinueIndexed => {
            assert(live_runtime_product_continue_indexed_step(before, after));
            let parent_source = product_admission_source_image(old.indexed_regions.last());
            match before.source.wire.last() {
                Some(reader) => match wire_source::wire_source_next_field(reader) {
                    wire_source::WireSourceTransition::Field { field, .. } =>
                        product_message_continue_preserves_ancestor_prefix(
                            old, new,
                            admission::ConcreteFrameActionModel::ContinueIndexed {
                                field, parent_source,
                            },
                        ),
                    _ => assert(false),
                },
                _ => assert(false),
            }
        },
        LiveRuntimeProductAction::MapKey => {
            assert(live_runtime_product_map_key_step(before, after));
            match before.source.wire.last() {
                Some(reader) => match wire_source::wire_source_next_field(reader) {
                    wire_source::WireSourceTransition::Field { field, .. } =>
                        product_map_key_preserves_ancestor_prefix(old, new, field),
                    _ => assert(false),
                },
                _ => assert(false),
            }
        },
        LiveRuntimeProductAction::RootScore => {
            assert(live_runtime_product_root_score_step(before, after));
            score_bridge::bridge_score_source_step_preserves_shape(
                before.source.product.bridge, after.source.product.bridge,
            );
        },
        LiveRuntimeProductAction::RootComplete { returned } => {
            assert(live_runtime_product_root_complete_step(before, after, returned));
            score_bridge::bridge_score_source_step_preserves_shape(
                before.source.product.bridge, after.source.product.bridge,
            );
        },
        LiveRuntimeProductAction::MessageChildScore => {
            assert(live_runtime_product_message_child_score_step(before, after));
            product_message_child_score_preserves_ancestor_prefix(old, new);
        },
        LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => {
            assert(live_runtime_product_message_push_observed_step(
                before, after, request, actual_top_kind, after_len, after_capacity,
                stats_frame_variant_bytes,
            ));
            if let runtime_source::SourceFramePushObservationResult::Success { storage } =
                runtime_source::source_frame_push_observe_after(
                    request.request_kind, actual_top_kind,
                    old.bridge.admission.frames.len() as usize,
                    before.source.runtime.capacity, after_len, after_capacity,
                    before.source.runtime.frame_bytes, stats_frame_variant_bytes,
                ) {
                if let Some(wire_source::WireConcreteTransition::Field { field, .. }) =
                    wire_source::wire_runtime_next_field(before.cursors.last().unwrap()) {
                    if live_runtime_product_message_push_step(before, after, request, storage) {
                        product_message_push_preserves_ancestor_prefix(
                            old, new, admission::ConcreteFrameActionModel::Push {
                                field, child: request.child,
                                frame_bytes: before.source.runtime.frame_bytes,
                            },
                        );
                    } else {
                        assert(live_runtime_product_message_push_indexed_step(
                            before, after, request, storage,
                        ));
                        product_message_push_preserves_ancestor_prefix(
                            old, new, admission::ConcreteFrameActionModel::PushIndexed {
                                field, child: request.child,
                                parent_source: product_admission_source_image(
                                    old.indexed_regions.last(),
                                ),
                                frame_bytes: before.source.runtime.frame_bytes,
                            },
                        );
                    }
                }
            }
        },
        LiveRuntimeProductAction::MessageEpmPush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => {
            assert(live_runtime_product_message_epm_push_indexed_observed_step(
                before, after, request, actual_top_kind, after_len, after_capacity,
                stats_frame_variant_bytes,
            ));
            if let runtime_source::SourceFramePushObservationResult::Success { storage } =
                runtime_source::source_frame_push_observe_after(
                    request.request_kind, actual_top_kind,
                    old.bridge.admission.frames.len() as usize,
                    before.source.runtime.capacity, after_len, after_capacity,
                    before.source.runtime.frame_bytes, stats_frame_variant_bytes,
                ) {
                assert(live_runtime_product_message_epm_push_indexed_step(
                    before, after, request, storage,
                ));
                if let Some(wire_source::WireConcreteTransition::Field { field, .. }) =
                    wire_source::wire_runtime_next_field(before.cursors.last().unwrap()) {
                    product_message_push_preserves_ancestor_prefix(
                        old, new, admission::ConcreteFrameActionModel::PushIndexed {
                            field, child: request.child,
                            parent_source: product_admission_source_image(
                                old.indexed_regions.last(),
                            ),
                            frame_bytes: before.source.runtime.frame_bytes,
                        },
                    );
                }
            }
        },
        LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => {
            assert(live_runtime_product_map_value_push_observed_step(
                before, after, request, actual_top_kind, after_len, after_capacity,
                stats_frame_variant_bytes,
            ));
            if let runtime_source::SourceFramePushObservationResult::Success { storage } =
                runtime_source::source_frame_push_observe_after(
                    request.request_kind, actual_top_kind,
                    old.bridge.admission.frames.len() as usize,
                    before.source.runtime.capacity, after_len, after_capacity,
                    before.source.runtime.frame_bytes, stats_frame_variant_bytes,
                ) {
                if let Some(wire_source::WireConcreteTransition::Field { field, .. }) =
                    wire_source::wire_runtime_next_field(before.cursors.last().unwrap()) {
                    if live_runtime_product_map_value_push_step(before, after, request, storage) {
                        product_map_value_push_preserves_ancestor_prefix(
                            old, new, map::IntegratedAdmissionAction::MapValue {
                                field, child: request.child,
                                frame_bytes: before.source.runtime.frame_bytes,
                            },
                        );
                    } else {
                        assert(live_runtime_product_map_value_push_indexed_step(
                            before, after, request, storage,
                        ));
                        product_map_value_push_preserves_ancestor_prefix(
                            old, new, map::IntegratedAdmissionAction::MapValueIndexed {
                                field, child: request.child,
                                parent_source: product_admission_source_image(
                                    old.indexed_regions.last(),
                                ),
                                frame_bytes: before.source.runtime.frame_bytes,
                            },
                        );
                    }
                }
            }
        },
        LiveRuntimeProductAction::MessageChildComplete => {
            assert(live_runtime_product_message_child_complete_step(before, after));
            product_map_pop_preserves_ancestor_prefix(
                old, new, map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::Complete,
                },
            );
        },
        LiveRuntimeProductAction::MapValueAccept => {
            assert(live_runtime_product_map_value_accept_step(before, after));
            product_map_pop_preserves_ancestor_prefix(
                old, new, map::IntegratedAdmissionAction::MapAccept,
            );
        },
        LiveRuntimeProductAction::MapEntryFinish => {
            reveal(live_runtime_product_map_entry_finish_step);
            assert(live_runtime_product_map_entry_finish_step(before, after));
            product_map_pop_preserves_ancestor_prefix(
                old, new, map::IntegratedAdmissionAction::MapFinish,
            );
        },
        LiveRuntimeProductAction::EpmMapValueAccept {
            summary, admission_summary,
        } => {
            assert(live_runtime_product_epm_map_value_accept_step(
                before, after, summary, admission_summary,
            ));
            product_epm_child_return_preserves_ancestor_prefix(
                old, new, summary, admission_summary,
            );
        },
        LiveRuntimeProductAction::EpmContinue { event } => {
            assert(rpb3_epm_unwind_product_lift::live_runtime_internal_epm_unwind_step(
                before, after, event,
            ));
            product_epm_continue_preserves_ancestor_prefix(old, new, event);
        },
        LiveRuntimeProductAction::EpmPush { observed } => {
            assert(rpb3_epm_push_live_action_refinement::epm_push_live_after(
                before, observed,
            ) == Some(after));
            live_epm_push_preserves_ancestor_prefix(before, after, observed);
        },
        LiveRuntimeProductAction::EpmEnd { observed } => {
            assert(live_runtime_product_epm_end_source_step(before, after, observed));
            let prior = product_linked_top(old).unwrap();
            let complete =
                rpb3_epathmap_terminal_source_observation::epm_terminal_complete_from_source(
                    prior.epm, observed,
                ).unwrap();
            product_epm_complete_preserves_ancestor_prefix(
                old, new, complete.action,
            );
        },
        LiveRuntimeProductAction::EpmFinishAccept => {
            assert(live_runtime_product_epm_finish_accept_step(before, after));
            product_epm_finish_accept_preserves_ancestor_prefix(old, new);
        },
    }
    assert(0 <= limit);
    assert(limit <= new.bridge.admission.frames.len());
    assert forall|i: int| 0 <= i < limit implies
        product_ancestor_view_same(old, new, i) by {
        match action {
            LiveRuntimeProductAction::RootScore
            | LiveRuntimeProductAction::RootComplete { .. } => assert(false),
            _ => {},
        }
    }
    if let LiveRuntimeProductAction::EpmPush { .. } = action {
    } else {
        assert(strict_ancestor_sequence_shape(
            before.source.wire, after.source.wire,
        ));
        assert(strict_ancestor_sequence_shape(before.cursors, after.cursors));
        live_wire_cursor_shape_preserves_prefix(before, after, limit);
    }
    assert forall|i: int| 0 <= i < limit implies
        live_runtime_product_ancestor_view_same(before, after, i) by {
        match action {
            LiveRuntimeProductAction::EpmPush { .. } => {},
            _ => {
                assert(product_ancestor_view_same(old, new, i));
                assert(after.source.product.map.admission.frames[i]
                    == before.source.product.map.admission.frames[i]);
                assert(after.source.runtime.score.admission.frames[i]
                    == before.source.runtime.score.admission.frames[i]);
                assert(after.source.wire[i] == before.source.wire[i]);
                assert(after.cursors[i] == before.cursors[i]);
            },
        }
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn live_runtime_product_trace(
    states: Seq<LiveRuntimeProductState>,
    actions: Seq<LiveRuntimeProductAction>,
) -> bool {
    &&& states.len() == actions.len() + 1
    &&& live_runtime_product_wf(states[0])
    &&& forall|i: int| 0 <= i < actions.len()
        ==> live_runtime_product_step(states[i], states[i + 1], actions[i])
}

/// A CanonicalPath driver action retains its parser beside the same live
/// admission product used by Message, Map, and EPathMap actions. The path
/// selector computes the next product from the indexed scan and checked
/// storage observation; the caller cannot choose an unrelated post-product.
pub struct LiveRuntimeRetainedPathState {
    pub ghost path: rpb3_canonical_path_driver_product_integration::CanonicalPathDriverProductState,
    pub ghost product: LiveRuntimeProductState,
}

pub open spec fn live_runtime_retained_path_wf(
    state: LiveRuntimeRetainedPathState,
) -> bool {
    &&& rpb3_canonical_path_driver_product_integration::canonical_path_driver_product_wf(
        state.path,
    )
    &&& state.product == state.path.product
}

/// Root Complete has no live post-frame. Push and ChildReturn retain the
/// parser/product pair; RootComplete returns the exact no-parent image.
pub enum LiveRuntimeRetainedPathOutcome {
    Active { state: LiveRuntimeRetainedPathState },
    RootComplete {
        returned: rpb3_live_canonical_path_terminal_refinement::LiveCanonicalPathTerminalOutcome,
    },
}

pub open spec fn live_runtime_retained_path_step(
    before: LiveRuntimeRetainedPathState,
    action: rpb3_canonical_path_driver_product_integration::CanonicalPathDriverSourceAction,
) -> Option<LiveRuntimeRetainedPathOutcome> {
    if !live_runtime_retained_path_wf(before) {
        None
    } else {
        match rpb3_canonical_path_driver_product_integration::canonical_path_driver_product_step(
            before.path, action,
        ) {
            None => None,
            Some(
                rpb3_canonical_path_driver_product_integration::CanonicalPathDriverProductOutcome::Active {
                    state,
                },
            ) => Some(LiveRuntimeRetainedPathOutcome::Active {
                state: LiveRuntimeRetainedPathState {
                    path: state,
                    product: state.product,
                },
            }),
            Some(
                rpb3_canonical_path_driver_product_integration::CanonicalPathDriverProductOutcome::Terminal {
                    returned,
                },
            ) => Some(LiveRuntimeRetainedPathOutcome::RootComplete { returned }),
        }
    }
}

pub proof fn live_runtime_retained_path_step_preserves_sync(
    before: LiveRuntimeRetainedPathState,
    action: rpb3_canonical_path_driver_product_integration::CanonicalPathDriverSourceAction,
)
    requires live_runtime_retained_path_wf(before),
    ensures match live_runtime_retained_path_step(before, action) {
        Some(LiveRuntimeRetainedPathOutcome::Active { state }) => {
            &&& live_runtime_retained_path_wf(state)
            &&& state.path.canonical_events == before.path.canonical_events + 1
            &&& state.path.last_driver_step == Some(
                rpb3_canonical_path_driver_product_integration::canonical_path_driver_action_storage(
                    action,
                ).step_index,
            )
        },
        Some(LiveRuntimeRetainedPathOutcome::RootComplete { .. }) => {
            &&& before.product.source.product.bridge.admission.frames.len() == 1
            &&& rpb3_canonical_path_driver_product_integration::canonical_path_driver_action_storage(
                action,
            ).after_len == 0
        },
        None => true,
    },
{
    rpb3_canonical_path_driver_product_integration::canonical_path_driver_product_active_preserves_wf(
        before.path, action,
    );
    rpb3_canonical_path_driver_product_integration::canonical_path_driver_product_active_sync(
        before.path, action,
    );
    match rpb3_canonical_path_driver_product_integration::canonical_path_driver_product_step(
        before.path, action,
    ) {
        Some(
            rpb3_canonical_path_driver_product_integration::CanonicalPathDriverProductOutcome::Active {
                state,
            },
        ) => {
            assert(rpb3_canonical_path_driver_product_integration::canonical_path_driver_product_wf(
                state,
            ));
            assert(live_runtime_retained_path_wf(LiveRuntimeRetainedPathState {
                path: state,
                product: state.product,
            }));
        },
        Some(
            rpb3_canonical_path_driver_product_integration::CanonicalPathDriverProductOutcome::Terminal {
                ..
            },
        ) => {
            let observed =
                rpb3_canonical_path_driver_product_integration::canonical_path_driver_action_storage(
                    action,
                );
            assert(rpb3_canonical_path_driver_product_integration::driver_storage_before_matches(
                before.path, observed,
            ));
            assert(observed.before_len == 1);
            assert(before.product == before.path.product);
            assert(before.product.source.product.bridge.admission.frames.len() == 1);
        },
        None => {},
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_singleton_trace(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
)
    requires live_runtime_product_wf(before),
        live_runtime_product_step(before, after, action),
    ensures live_runtime_product_trace(seq![before, after], seq![action]),
{
    let states = seq![before, after];
    let actions = seq![action];
    assert forall|i: int| 0 <= i < actions.len() implies
        live_runtime_product_step(states[i], states[i + 1], actions[i]) by {
        assert(i == 0);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_trace_prefix_wf(
    states: Seq<LiveRuntimeProductState>,
    actions: Seq<LiveRuntimeProductAction>,
    index: int,
)
    requires live_runtime_product_trace(states, actions),
        0 <= index < states.len(),
    ensures live_runtime_product_wf(states[index]),
    decreases index,
{
    if index > 0 {
        live_runtime_product_trace_prefix_wf(states, actions, index - 1);
        live_runtime_product_step_preserves_wf(
            states[index - 1], states[index], actions[index - 1],
        );
    }
}

// A production message EOF scores the child and then accepts it in one driver
// call.  The two live-product actions refine that one observable transition.
#[cfg(rpb3_live_push)]
pub proof fn live_runtime_message_child_complete_map_dispatch(
    scored: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_message_child_complete_step(scored, after),
    ensures
        admission::concrete_message_driver_step(
            scored.source.product.bridge.admission,
            admission::ConcreteFrameActionModel::Complete,
        ) == Some(admission::ConcreteFrameOutcomeModel::ParentAccepted {
            state: after.source.product.bridge.admission,
        }),
        after.source.product.bridge.transcript
            == scored.source.product.bridge.transcript,
{
    let complete = map::IntegratedAdmissionAction::Message {
        action: admission::ConcreteFrameActionModel::Complete,
    };
    assert(product_map_step(
        scored.source.product, after.source.product, complete,
    ));
    assert(map::integrated_admission_driver_step(
        scored.source.product.map, complete,
    ) == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
        state: after.source.product.map,
    }));
    map::integrated_message_parent_accept_pop_shape(
        scored.source.product.map, after.source.product.map,
    );
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_message_child_complete_pre_pop_exact(
    before: LiveRuntimeProductState,
    scored: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        live_runtime_product_message_child_score_step(before, scored),
        live_runtime_product_message_child_complete_step(scored, after),
        runtime_source::runtime_child_eof_gate(
            before.source.runtime, scored.source.product.bridge,
        ),
        admission::concrete_message_driver_step(
            scored.source.product.bridge.admission,
            admission::ConcreteFrameActionModel::Complete,
        ) == Some(admission::ConcreteFrameOutcomeModel::ParentAccepted {
            state: after.source.product.bridge.admission,
        }),
        after.source.product.bridge.transcript
            == scored.source.product.bridge.transcript,
    ensures runtime_source::runtime_message_eof_child_accept(
        before.source.runtime, scored.source.product.bridge,
    ) == Some(after.source.runtime),
{
    let pre_pop = runtime_source::RuntimeFrameImage {
        score: scored.source.product.bridge,
        ..before.source.runtime
    };
    assert(pre_pop == scored.source.runtime);
    assert(after.source.runtime == runtime_source::runtime_after_admission_step(
        pre_pop, after.source.product.bridge.admission,
    ));
    reveal(runtime_source::runtime_child_eof_gate);
    assert(runtime_source::runtime_frame_image_wf(pre_pop));
    reveal(runtime_source::runtime_pre_pop_child_accept);
    reveal(runtime_source::runtime_message_eof_child_accept);
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_child_complete_runtime_exact(
    before: LiveRuntimeProductState,
    scored: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        live_runtime_product_message_child_score_step(before, scored),
        live_runtime_product_message_child_complete_step(scored, after),
    ensures runtime_source::runtime_message_eof_child_accept(
        before.source.runtime, scored.source.product.bridge,
    ) == Some(after.source.runtime),
{
    live_runtime_product_message_child_score_enables_runtime_gate(before, scored);
    live_runtime_message_child_complete_map_dispatch(scored, after);
    live_runtime_message_child_complete_pre_pop_exact(before, scored, after);
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_product_message_child_eof_driver_exact(
    before: LiveRuntimeProductState,
    scored: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        live_runtime_product_message_child_score_step(before, scored),
        live_runtime_product_message_child_complete_step(scored, after),
    ensures
        runtime_source::runtime_message_driver_step(
            before.source.runtime, runtime_source::RuntimeMessageEvent::Eof,
        ) == Some(runtime_source::RuntimeMessageOutcome::ParentAccepted {
            image: after.source.runtime,
        }),
{
    live_runtime_product_message_child_score_enables_runtime_gate(before, scored);
    live_runtime_product_message_child_complete_runtime_exact(
        before, scored, after,
    );
    assert(before.source.runtime.score == before.source.product.bridge);
    assert(score_bridge::bridge_score_finish_selected(before.source.runtime.score)
        == Some(scored.source.product.bridge));
    assert(runtime_source::runtime_message_eof_child_accept(
        before.source.runtime, scored.source.product.bridge,
    ) == Some(after.source.runtime));
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_empty_send_initial() -> Option<LiveRuntimeProductState> {
    let root = finish::empty_send_finish_root();
    let indexed_backing = epm::IndexedBackingImage::Borrowed {
        bytes: Seq::empty(),
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 0,
        end_override: None,
    };
    let wire_backing = wire_source::WireRuntimeBackingImage::Borrowed {
        bytes: Seq::empty(),
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 0,
        end_override: None,
    };
    match (
        product_root_initial(root, indexed_backing),
        wire_source::wire_runtime_initial(wire_backing, false),
    ) {
        (Some(product), Some(cursor)) =>
            match wire_source::wire_runtime_cursor_project(cursor) {
                Some(reader) => Some(LiveRuntimeProductState {
                    source: LiveSourceProductState {
                        product,
                        runtime: runtime_source::runtime_empty_send_before(),
                        arena: arena_source::RuntimeScoreArena { entries: Seq::empty() },
                        wire: seq![Some(reader)],
                    },
                    cursors: seq![Some(cursor)],
                }),
                None => None,
            },
        _ => None,
    }
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_empty_send_initial_is_wf()
    ensures
        live_runtime_empty_send_initial().is_some(),
        live_runtime_product_wf(live_runtime_empty_send_initial().unwrap()),
{
    let root = finish::empty_send_finish_root();
    let indexed_backing = epm::IndexedBackingImage::Borrowed {
        bytes: Seq::empty(),
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 0,
        end_override: None,
    };
    assert(admission::admission_root_initial(root));
    map::integrated_initial_is_well_formed(root);
    assert(epm::indexed_backing_project(indexed_backing).is_some());
    let indexed = epm::indexed_backing_project(indexed_backing).unwrap();
    assert(epm::indexed_image_wf(indexed));
    assert(indexed.bytes =~= root.source_bytes);
    assert(product_root_initial(root, indexed_backing).is_some());
    product_root_initial_exact(root, indexed_backing);
    admission::admission_initial_wf(root);
    score_bridge::bridge_initial_exact(root);
    let state = live_runtime_empty_send_initial().unwrap();
    assert(state.source.product.bridge == runtime_source::runtime_empty_send_before().score);
    assert(runtime_source::runtime_frame_image_wf(state.source.runtime));
    assert(arena_source::runtime_projects_score(
        state.source.arena, state.source.product.bridge.transcript,
    ));
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_empty_send_root_end_is_ready()
    ensures
        live_runtime_empty_send_initial().is_some(),
        wire_source::wire_runtime_next_field(
            live_runtime_empty_send_initial().unwrap().cursors[0].unwrap(),
        ) == Some(wire_source::WireConcreteTransition::End {
            state: wire_source::wire_runtime_derive_cursor(
                live_runtime_empty_send_initial().unwrap().cursors[0].unwrap(),
            ).unwrap(),
        }),
        finish::message_eof_ready(
            live_runtime_empty_send_initial().unwrap().source.product.bridge,
        ),
{
    live_runtime_empty_send_initial_is_wf();
    let state = live_runtime_empty_send_initial().unwrap();
    let cursor = state.cursors[0].unwrap();
    assert(wire_source::wire_runtime_next_field(cursor)
        == Some(wire_source::WireConcreteTransition::End {
            state: wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
        }));
    live_runtime_product_root_end_is_ready(state);
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_empty_send_source_finish_is_reachable()
    ensures
        live_runtime_empty_send_initial().is_some(),
        finish::empty_send_finish_after().is_some(),
        score_bridge::bridge_score_finish_selected(
            live_runtime_empty_send_initial().unwrap().source.runtime.score,
        ) == finish::empty_send_finish_after(),
        match runtime_source::runtime_message_driver_step(
            live_runtime_empty_send_initial().unwrap().source.runtime,
            runtime_source::RuntimeMessageEvent::Eof,
        ) {
            Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }) => {
                &&& returned.remaining_frames == 0
                &&& returned.summary.family == admission::AdmissionFamily::Send
                &&& !returned.summary.complete
                &&& returned.summary.score_handle == 1
            },
            _ => false,
        },
{
    live_runtime_empty_send_root_end_is_ready();
    runtime_source::runtime_selected_empty_send_return_is_reachable();
    let before = live_runtime_empty_send_initial().unwrap().source.runtime;
    assert(before.score == finish::empty_send_finish_before());
    let root = finish::empty_send_finish_root();
    assert(admission::admission_root_initial(root));
    assert(admission::admission_frame_wf(root));
    assert(score_bridge::bridge_primary_component_frame(before.score));
    score_bridge::bridge_primary_component_finish_selected_exact(before.score);
    assert(!admission::admission_family_complete(root));
    assert(score_bridge::bridge_send_step(before.score).is_none());
    assert(score_bridge::bridge_primary_component_finish(before.score).is_none());
    assert(score_bridge::bridge_nonsemantic_finish_candidate(before.score)
        == finish::empty_send_finish_after());
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_empty_send_scored_product() -> Option<ProductScoreState> {
    match (
        live_runtime_empty_send_initial(),
        finish::empty_send_finish_after(),
    ) {
        (Some(before), Some(scored)) => {
            let prior = before.source.product;
            Some(ProductScoreState {
                bridge: scored,
                map: map::IntegratedAdmissionState {
                    admission: scored.admission,
                    map_frames: prior.map.map_frames,
                    transcript: scored.transcript,
                },
                epm_frames: prior.epm_frames,
                indexed_regions: prior.indexed_regions,
                accepted_epm_sources: prior.accepted_epm_sources,
            })
        },
        _ => None,
    }
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_empty_send_bridge_source_is_exact()
    ensures
        finish::empty_send_finish_after().is_some(),
        score_bridge::bridge_score_source_step(
            finish::empty_send_finish_before(),
            finish::empty_send_finish_after().unwrap(),
        ),
{
    finish::empty_send_finish_has_root_return();
    let before = finish::empty_send_finish_before();
    let after = finish::empty_send_finish_after().unwrap();
    assert(finish::modeled_message_eof_to_root_return(before, after).is_some());
    assert(finish::modeled_message_finish_action(before, after).is_some());
    assert(score_bridge::bridge_score_source_step(before, after));
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_empty_send_product_root_step_is_exact()
    ensures
        live_runtime_empty_send_scored_product().is_some(),
        product_root_score_step(
            live_runtime_empty_send_initial().unwrap().source.product,
            live_runtime_empty_send_scored_product().unwrap(),
        ),
{
    live_runtime_empty_send_initial_is_wf();
    live_runtime_empty_send_bridge_source_is_exact();
    let before = live_runtime_empty_send_initial().unwrap().source.product;
    let after = live_runtime_empty_send_scored_product().unwrap();
    assert(before.bridge == finish::empty_send_finish_before());
    assert(after.bridge == finish::empty_send_finish_after().unwrap());
    assert(score_bridge::bridge_score_source_step(before.bridge, after.bridge));
    assert(before.epm_frames[0].is_none());
    assert(after.accepted_epm_sources == product_epm_receipts_carried(before, after));
    assert(product_root_score_step(before, after));
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_empty_send_scored_state()
    -> Option<LiveRuntimeProductState> {
    match (
        live_runtime_empty_send_initial(),
        live_runtime_empty_send_scored_product(),
    ) {
        (Some(before), Some(product)) => Some(LiveRuntimeProductState {
            source: LiveSourceProductState {
                product,
                runtime: runtime_source::RuntimeFrameImage {
                    score: product.bridge,
                    ..before.source.runtime
                },
                arena: product_runtime_arena_from_score(product.bridge.transcript),
                wire: before.source.wire,
            },
            cursors: before.cursors,
        }),
        _ => None,
    }
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_empty_send_root_score_live_step_is_exact()
    ensures
        live_runtime_empty_send_scored_state().is_some(),
        live_runtime_product_root_score_step(
            live_runtime_empty_send_initial().unwrap(),
            live_runtime_empty_send_scored_state().unwrap(),
        ),
{
    live_runtime_empty_send_initial_is_wf();
    live_runtime_empty_send_root_end_is_ready();
    live_runtime_empty_send_source_finish_is_reachable();
    live_runtime_empty_send_product_root_step_is_exact();
    let before = live_runtime_empty_send_initial().unwrap();
    let after = live_runtime_empty_send_scored_state().unwrap();
    assert(live_runtime_product_root_score_step(before, after));
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_empty_send_root_score_live_after_is_wf()
    ensures
        live_runtime_empty_send_scored_state().is_some(),
        live_runtime_product_wf(live_runtime_empty_send_scored_state().unwrap()),
{
    live_runtime_empty_send_root_score_live_step_is_exact();
    let before = live_runtime_empty_send_initial().unwrap();
    let after = live_runtime_empty_send_scored_state().unwrap();
    live_runtime_product_root_score_preserves_wf(before, after);
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_empty_send_root_score_live_trace_is_exact()
    ensures
        live_runtime_product_trace(
            seq![
                live_runtime_empty_send_initial().unwrap(),
                live_runtime_empty_send_scored_state().unwrap(),
            ],
            seq![LiveRuntimeProductAction::RootScore],
        ),
{
    live_runtime_empty_send_root_score_live_after_is_wf();
    live_runtime_empty_send_root_score_live_step_is_exact();
    let before = live_runtime_empty_send_initial().unwrap();
    let after = live_runtime_empty_send_scored_state().unwrap();
    assert(live_runtime_product_root_score_step(before, after));
    assert(live_runtime_product_step(before, after, LiveRuntimeProductAction::RootScore));
    live_runtime_product_singleton_trace(before, after, LiveRuntimeProductAction::RootScore);
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_empty_send_root_score_is_reachable()
    ensures
        live_runtime_empty_send_scored_product().is_some(),
        product_root_score_step(
            live_runtime_empty_send_initial().unwrap().source.product,
            live_runtime_empty_send_scored_product().unwrap(),
        ),
        product_source_step(
            live_runtime_empty_send_initial().unwrap().source.product,
            live_runtime_empty_send_scored_product().unwrap(),
            ProductSourceAction::RootScore,
        ),
        product_score_wf(live_runtime_empty_send_scored_product().unwrap()),
{
    live_runtime_empty_send_product_root_step_is_exact();
    let before = live_runtime_empty_send_initial().unwrap().source.product;
    let after = live_runtime_empty_send_scored_product().unwrap();
    assert(product_source_step(before, after, ProductSourceAction::RootScore));
    product_root_score_step_preserves_exact(before, after);
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_empty_send_root_score_trace_is_exact()
    ensures
        product_root_trace(
            finish::empty_send_finish_root(),
            epm::IndexedBackingImage::Borrowed {
                bytes: Seq::empty(),
                source: 0,
                absolute_start: 0,
                start: 0,
                len: 0,
                end_override: None,
            },
            seq![
                live_runtime_empty_send_initial().unwrap().source.product,
                live_runtime_empty_send_scored_product().unwrap(),
            ],
            seq![ProductSourceAction::RootScore],
        ),
        product_score_wf(live_runtime_empty_send_scored_product().unwrap()),
{
    live_runtime_empty_send_root_score_is_reachable();
    let root = finish::empty_send_finish_root();
    let backing = epm::IndexedBackingImage::Borrowed {
        bytes: Seq::empty(),
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 0,
        end_override: None,
    };
    assert(product_root_initial(root, backing)
        == Some(live_runtime_empty_send_initial().unwrap().source.product));
    let states = seq![
        live_runtime_empty_send_initial().unwrap().source.product,
        live_runtime_empty_send_scored_product().unwrap(),
    ];
    let actions = seq![ProductSourceAction::RootScore];
    assert(product_root_trace(root, backing, states, actions));
    product_root_trace_prefix_exact(root, backing, states, actions, 1);
}

#[cfg(all(rpb3_terminal, rpb3_live_witness))]
pub proof fn live_runtime_empty_send_complete_is_exact(claimed: Seq<u8>)
    ensures
        live_runtime_empty_send_initial().is_some(),
        live_runtime_empty_send_scored_product().is_some(),
        match runtime_source::runtime_message_driver_step(
            live_runtime_empty_send_initial().unwrap().source.runtime,
            runtime_source::RuntimeMessageEvent::Eof,
        ) {
            Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }) => {
                match map::integrated_admission_driver_step(
                    live_runtime_empty_send_scored_product().unwrap().map,
                    map::IntegratedAdmissionAction::Message {
                        action: admission::ConcreteFrameActionModel::Complete,
                    },
                ) {
                    Some(map::IntegratedAdmissionOutcome::RootFinished { root_summary }) => {
                        let scored = live_runtime_empty_send_scored_product().unwrap().bridge;
                        let handle = root_summary.score_handle;
                        &&& root_summary == returned.summary
                        &&& returned.score_bytes
                            == scored.admission.arena[handle as int].transcript_bytes
                        &&& score_bridge::bridge_root_first_difference(scored, claimed).is_none()
                            == rpb3_score_transcript::claimed_score_matches(
                                scored.transcript, handle, claimed,
                            )
                    },
                    _ => false,
                }
            },
            _ => false,
        },
{
    live_runtime_empty_send_root_end_is_ready();
    live_runtime_empty_send_root_score_is_reachable();
    let before = live_runtime_empty_send_initial().unwrap();
    let scored = live_runtime_empty_send_scored_product().unwrap();
    live_runtime_empty_send_source_finish_is_reachable();
    assert(score_bridge::bridge_score_finish_selected(before.source.runtime.score)
        == Some(scored.bridge));
    if let Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }) =
        runtime_source::runtime_message_driver_step(
            before.source.runtime, runtime_source::RuntimeMessageEvent::Eof,
        )
    {
        live_runtime_root_complete_matches_source(
            before, scored, returned, claimed,
        );
    }
}

#[cfg(all(rpb3_terminal, rpb3_live_witness))]
pub proof fn live_runtime_empty_send_root_complete_live_trace_is_exact(
    claimed: Seq<u8>,
)
    ensures match runtime_source::runtime_message_driver_step(
        live_runtime_empty_send_initial().unwrap().source.runtime,
        runtime_source::RuntimeMessageEvent::Eof,
    ) {
        Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }) => {
            &&& live_runtime_product_trace(
                seq![
                    live_runtime_empty_send_initial().unwrap(),
                    live_runtime_empty_send_scored_state().unwrap(),
                ],
                seq![LiveRuntimeProductAction::RootComplete { returned }],
            )
            &&& returned.remaining_frames == 0
            &&& returned.pre_pop
                == live_runtime_empty_send_scored_state().unwrap().source.runtime
            &&& match map::integrated_admission_driver_step(
                live_runtime_empty_send_scored_state().unwrap().source.product.map,
                map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::Complete,
                },
            ) {
                Some(map::IntegratedAdmissionOutcome::RootFinished { root_summary }) => {
                    let scored = live_runtime_empty_send_scored_state().unwrap()
                        .source.product.bridge;
                    let handle = root_summary.score_handle;
                    &&& root_summary == returned.summary
                    &&& score_bridge::bridge_root_first_difference(scored, claimed).is_none()
                        == rpb3_score_transcript::claimed_score_matches(
                            scored.transcript, handle, claimed,
                        )
                },
                _ => false,
            }
        },
        _ => false,
    },
{
    live_runtime_empty_send_root_score_live_step_is_exact();
    live_runtime_empty_send_complete_is_exact(claimed);
    let before = live_runtime_empty_send_initial().unwrap();
    let after = live_runtime_empty_send_scored_state().unwrap();
    assert(before.source.product.bridge.admission.frames[0].frame_kind == 1);
    assert(before.source.product.bridge.admission.frames[0].waiting_id.is_none());
    assert(before.source.product.bridge.admission.frames[0].score_handle.is_none());
    if let Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }) =
        runtime_source::runtime_message_driver_step(
            before.source.runtime, runtime_source::RuntimeMessageEvent::Eof,
        )
    {
        assert(live_runtime_product_root_complete_step(before, after, returned));
        live_runtime_product_singleton_trace(
            before, after, LiveRuntimeProductAction::RootComplete { returned },
        );
        live_runtime_product_root_complete_is_exact(before, after, returned, claimed);
    }
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_send_channel_root() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        source_bytes: seq![0x0au8, 0x00u8],
        region_end: 2,
        ..finish::empty_send_finish_root()
    }
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_send_channel_initial(
    frame_bytes: usize,
) -> Option<LiveRuntimeProductState> {
    let root = live_runtime_send_channel_root();
    let indexed_backing = epm::IndexedBackingImage::Borrowed {
        bytes: root.source_bytes,
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 2,
        end_override: None,
    };
    match (
        product_root_initial(root, indexed_backing),
        wire_source::wire_runtime_initial(
            wire_source::wire_empty_par_field_backing(), false,
        ),
    ) {
        (Some(product), Some(cursor)) =>
            match wire_source::wire_runtime_cursor_project(cursor) {
                Some(reader) => Some(LiveRuntimeProductState {
                    source: LiveSourceProductState {
                        product,
                        runtime: runtime_source::RuntimeFrameImage {
                            score: score_bridge::bridge_initial(root),
                            capacity: 1,
                            frame_bytes,
                            max_frames: 1,
                            max_storage_bytes: frame_bytes,
                            max_capacity_bytes: frame_bytes,
                        },
                        arena: arena_source::RuntimeScoreArena { entries: Seq::empty() },
                        wire: seq![Some(reader)],
                    },
                    cursors: seq![Some(cursor)],
                }),
                None => None,
            },
        _ => None,
    }
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_discontinuous_send_initial(
    frame_bytes: usize,
) -> Option<LiveRuntimeProductState> {
    let root = live_runtime_send_channel_root();
    let indexed_backing = epm::IndexedBackingImage::Borrowed {
        bytes: root.source_bytes,
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 2,
        end_override: Some(epm::SourceAnchor { source: 1, offset: 100 }),
    };
    let wire_backing = wire_source::WireRuntimeBackingImage::Borrowed {
        bytes: root.source_bytes,
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 2,
        end_override: Some(wire_source::WireSourceAnchor {
            source: 1, offset: 100,
        }),
    };
    match (
        product_root_initial(root, indexed_backing),
        wire_source::wire_runtime_initial(wire_backing, false),
    ) {
        (Some(product), Some(cursor)) =>
            match wire_source::wire_runtime_cursor_project(cursor) {
                Some(reader) => Some(LiveRuntimeProductState {
                    source: LiveSourceProductState {
                        product,
                        runtime: runtime_source::RuntimeFrameImage {
                            score: score_bridge::bridge_initial(root),
                            capacity: 1,
                            frame_bytes,
                            max_frames: 1,
                            max_storage_bytes: frame_bytes,
                            max_capacity_bytes: frame_bytes,
                        },
                        arena: arena_source::RuntimeScoreArena { entries: Seq::empty() },
                        wire: seq![Some(reader)],
                    },
                    cursors: seq![Some(cursor)],
                }),
                None => None,
            },
        _ => None,
    }
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_discontinuous_send_child()
    -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: 1,
        physical_start: 100,
        ..live_runtime_send_channel_child()
    }
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_indexed_message_push_after(
    before: LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
) -> Option<LiveRuntimeProductState> {
    if before.cursors.len() == 0
        || before.source.product.indexed_regions.len() == 0
    {
        None
    } else {
        match before.cursors.last() {
            None => None,
            Some(parent) => match wire_source::wire_runtime_next_field(parent) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field, start: _,
                }) => {
                    let parent_source = product_admission_source_image(
                        before.source.product.indexed_regions.last(),
                    );
                    let action = map::IntegratedAdmissionAction::Message {
                        action: admission::ConcreteFrameActionModel::PushIndexed {
                            field,
                            child: request.child,
                            parent_source,
                            frame_bytes: before.source.runtime.frame_bytes,
                        },
                    };
                    match (
                        map::integrated_admission_driver_step(
                            before.source.product.map, action,
                        ),
                        product_indexed_child(
                            before.source.product.indexed_regions.last(), field,
                        ),
                        runtime_source::source_checked_frame_request_push_indexed(
                            before.source.runtime, request, parent_source, storage,
                        ),
                        live_runtime_child_cursor(parent, field),
                    ) {
                        (
                            Some(map::IntegratedAdmissionOutcome::MessagePushed {
                                state: next_map,
                            }),
                            Some(child_region),
                            Some(next_runtime),
                            Some(child_cursor),
                        ) => {
                            let next_parent = runtime_wire_after_field(
                                parent, concrete_after,
                            );
                            match (
                                wire_source::wire_runtime_cursor_project(next_parent),
                                wire_source::wire_runtime_cursor_project(child_cursor),
                            ) {
                                (Some(next_parent_reader), Some(child_reader)) => {
                                    let product = ProductScoreState {
                                        bridge: score_bridge::ScoreAdmissionBridge {
                                            admission: next_map.admission,
                                            transcript: next_map.transcript,
                                        },
                                        map: next_map,
                                        epm_frames: before.source.product.epm_frames.push(None),
                                        indexed_regions:
                                            before.source.product.indexed_regions.push(
                                                child_region,
                                            ),
                                        accepted_epm_sources:
                                            before.source.product.accepted_epm_sources.push(None),
                                    };
                                    let after = LiveRuntimeProductState {
                                        source: LiveSourceProductState {
                                            product,
                                            runtime: next_runtime,
                                            arena: before.source.arena,
                                            wire: before.source.wire.update(
                                                before.source.wire.len() - 1,
                                                Some(next_parent_reader),
                                            ).push(Some(child_reader)),
                                        },
                                        cursors: before.cursors.update(
                                            before.cursors.len() - 1,
                                            Some(next_parent),
                                        ).push(Some(child_cursor)),
                                    };
                                    Some(after)
                                },
                                _ => None,
                            }
                        },
                        _ => None,
                    }
                },
                _ => None,
            },
        }
    }
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_discontinuous_send_initial_is_wf(frame_bytes: usize)
    requires frame_bytes > 0,
    ensures
        live_runtime_discontinuous_send_initial(frame_bytes).is_some(),
        live_runtime_product_wf(
            live_runtime_discontinuous_send_initial(frame_bytes).unwrap(),
        ),
        product_frame_source_anchors_match(
            live_runtime_discontinuous_send_initial(frame_bytes).unwrap()
                .source.product,
        ),
        product_wire_indexed_source_agrees(
            live_runtime_discontinuous_send_initial(frame_bytes).unwrap()
                .cursors[0].unwrap(),
            live_runtime_discontinuous_send_initial(frame_bytes).unwrap()
                .source.product.indexed_regions[0],
        ),
{
    let root = live_runtime_send_channel_root();
    let indexed_backing = epm::IndexedBackingImage::Borrowed {
        bytes: root.source_bytes,
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 2,
        end_override: Some(epm::SourceAnchor { source: 1, offset: 100 }),
    };
    let wire_backing = wire_source::WireRuntimeBackingImage::Borrowed {
        bytes: root.source_bytes,
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 2,
        end_override: Some(wire_source::WireSourceAnchor {
            source: 1, offset: 100,
        }),
    };
    assert(admission::admission_root_initial(root));
    map::integrated_initial_is_well_formed(root);
    assert(epm::indexed_backing_project(indexed_backing).is_some());
    let indexed = epm::indexed_backing_project(indexed_backing).unwrap();
    assert(epm::indexed_image_wf(indexed));
    assert(indexed.bytes =~= root.source_bytes);
    product_root_initial_exact(root, indexed_backing);
    admission::admission_initial_wf(root);
    score_bridge::bridge_initial_exact(root);
    let state = live_runtime_discontinuous_send_initial(frame_bytes).unwrap();
    assert(state.source.product.bridge == score_bridge::bridge_initial(root));
    product_frame_source_anchors_root(state.source.product);
    assert(runtime_source::runtime_frame_image_wf(state.source.runtime));
    assert(arena_source::runtime_projects_score(
        state.source.arena, state.source.product.bridge.transcript,
    ));
    assert(wire_source::wire_runtime_project_region(wire_backing).is_some());
    assert(product_wire_indexed_source_agrees(state.cursors[0].unwrap(), indexed));
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_discontinuous_send_admission_push_is_possible(
    frame_bytes: usize,
)
    requires
        frame_bytes > 0,
        2 * frame_bytes as int <= usize::MAX as int,
        2 <= usize::MAX as int / frame_bytes as int,
    ensures match live_runtime_discontinuous_send_initial(frame_bytes) {
        None => false,
        Some(before) => match map::integrated_admission_driver_step(
            before.source.product.map,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::PushIndexed {
                    field: admission::admission_discontinuous_empty_child_field(),
                    child: live_runtime_discontinuous_send_child(),
                    parent_source: product_admission_source_image(
                        before.source.product.indexed_regions.last(),
                    ),
                    frame_bytes,
                },
            },
        ) {
            Some(map::IntegratedAdmissionOutcome::MessagePushed { .. }) => true,
            _ => false,
        },
    },
{
    live_runtime_discontinuous_send_initial_is_wf(frame_bytes);
    admission::admission_discontinuous_empty_child_anchor_witness();
    let before = live_runtime_discontinuous_send_initial(frame_bytes).unwrap();
    let state = before.source.product.bridge.admission;
    let parent = state.frames[0];
    let field = admission::admission_discontinuous_empty_child_field();
    let child = live_runtime_discontinuous_send_child();
    let parent_source = product_admission_source_image(
        before.source.product.indexed_regions.last(),
    );
    assert(parent_source.bytes == parent.source_bytes);
    assert(admission::admission_indexed_source_wf(parent_source));
    assert(admission::admission_indexed_source_position(
        parent_source, field.payload_start,
    ) == Some(admission::AdmissionSourceAnchor { source: 1, offset: 100 }));
    assert(child.physical_source == 1);
    assert(parent.physical_source == 0);
    assert(admission::concrete_message_view(state).is_some());
    assert(admission::concrete_message_push(
        admission::concrete_message_view(state).unwrap(),
        field, child, frame_bytes,
    ).is_none());
    assert(child.physical_start == 100);
    assert(child.source_bytes == field.payload_bytes);
    assert(parent.source_bytes.subrange(2, 2) == child.source_bytes);
    assert(exists|start: int| 0 <= start
        && start + child.region_end as int <= parent.region_end as int
        && child.source_bytes == #[trigger] parent.source_bytes.subrange(
            start, start + child.region_end as int,
        )) by {
        assert(0 <= 2int);
        assert(2 + child.region_end as int <= parent.region_end as int);
        assert(child.source_bytes == parent.source_bytes.subrange(2, 2));
    }
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::indexed_field_at_cursor(
        admission::IndexedCanonicalFieldCursorModel {
            region_start: parent.region_start,
            region_end: parent.region_end,
            next: parent.cursor,
            source_bytes: parent.source_bytes,
        }, field,
    ));
    assert(admission::admission_push_step(
        state, field.encoded_end, child, 0, false, field.number, frame_bytes,
    ).is_some());
    admission::admission_push_preserves_wf(
        state, field.encoded_end, child, 0, false, field.number, frame_bytes,
    );
    let next_admission = admission::admission_push_step(
        state, field.encoded_end, child, 0, false, field.number, frame_bytes,
    ).unwrap();
    assert(admission::admission_state_wf(next_admission));
    assert(next_admission.frames.last().frame_kind == 1);
    assert(admission::concrete_message_push_indexed(
        admission::concrete_message_view(state).unwrap(),
        field, child, parent_source, frame_bytes,
    ) == Some(next_admission));
    assert(map::integrated_source_message_action(
        state,
        admission::ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source, frame_bytes,
        },
    ) == Some(admission::ConcreteFrameActionModel::PushIndexed {
        field, child, parent_source, frame_bytes,
    }));
    let next_map = map::IntegratedAdmissionState {
        admission: next_admission,
        map_frames: before.source.product.map.map_frames.push(None),
        transcript: before.source.product.map.transcript,
    };
    assert(map::integrated_admission_wf(next_map));
    assert(map::integrated_admission_result(
        next_admission,
        before.source.product.map.map_frames.push(None),
        before.source.product.map.transcript,
    ) == Some(next_map));
    assert(map::integrated_admission_driver_step(
        before.source.product.map,
        map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::PushIndexed {
                field, child, parent_source, frame_bytes,
            },
        },
    ) == Some(map::IntegratedAdmissionOutcome::MessagePushed { state: next_map }));
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_discontinuous_send_wire_field_is_read(frame_bytes: usize)
    requires frame_bytes > 0,
    ensures match live_runtime_discontinuous_send_initial(frame_bytes) {
        None => false,
        Some(before) => match wire_source::wire_runtime_next_field(
            before.cursors[0].unwrap(),
        ) {
            Some(wire_source::WireConcreteTransition::Field { field, .. }) => {
                &&& field == admission::admission_discontinuous_empty_child_field()
                &&& live_runtime_child_cursor(
                    before.cursors[0].unwrap(), field,
                ).is_some()
            },
            _ => false,
        },
    },
{
    live_runtime_discontinuous_send_initial_is_wf(frame_bytes);
    let before = live_runtime_discontinuous_send_initial(frame_bytes).unwrap();
    let cursor = before.cursors[0].unwrap();
    let region = wire_source::wire_runtime_project_region(cursor.message).unwrap();
    assert(region.bytes == seq![0x0au8, 0x00u8]);
    assert(region.end.source == 1);
    assert(region.end.offset == 100);
    assert(wire_source::wire_varint_read(region.bytes, 0, 2)
        == wire_source::WireVarintResult::Value { value: 10, next: 1 });
    assert(wire_source::wire_varint_read(region.bytes, 1, 2)
        == wire_source::WireVarintResult::Value { value: 0, next: 2 });
    let derived = wire_source::wire_runtime_derive_cursor(cursor).unwrap();
    assert(wire_source::wire_concrete_cursor_wf(derived));
    wire_source::wire_concrete_next_field_or_matches_model(derived);
    assert(wire_source::wire_runtime_next_field(cursor).is_some());
    if let Some(wire_source::WireConcreteTransition::Field {
        field, ..
    }) = wire_source::wire_runtime_next_field(cursor) {
        assert(field.number == 1);
        assert(field.wire == 2);
        assert(field.encoded_start == 0);
        assert(field.encoded_end == 2);
        assert(field.payload_start == 2);
        assert(field.payload_end == 2);
        assert(field.key_width == 1);
        assert(field.payload_width == 1);
        assert(field.payload_length == 0);
        assert(field.decoded_varint == 0);
        assert(field.payload_bytes == Seq::<u8>::empty());
        assert(field == admission::admission_discontinuous_empty_child_field());
        assert(live_runtime_child_cursor(cursor, field).is_some());
    }
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_discontinuous_send_indexed_push_reaches_live_state(
    frame_bytes: usize,
)
    requires
        frame_bytes > 0,
        2 * frame_bytes as int <= usize::MAX as int,
        2 <= usize::MAX as int / frame_bytes as int,
    ensures
        live_runtime_discontinuous_send_initial(frame_bytes).is_some(),
        live_runtime_indexed_message_push_after(
            live_runtime_discontinuous_send_initial(frame_bytes).unwrap(),
            runtime_source::SourceFrameRequestImage {
                request_kind: runtime_source::SourceFrameRequestKind::Message,
                field: admission::admission_discontinuous_empty_child_field(),
                child: live_runtime_discontinuous_send_child(),
            },
            live_runtime_send_channel_storage(frame_bytes),
        ).is_some(),
        live_runtime_product_message_push_indexed_step(
            live_runtime_discontinuous_send_initial(frame_bytes).unwrap(),
            live_runtime_indexed_message_push_after(
                live_runtime_discontinuous_send_initial(frame_bytes).unwrap(),
                runtime_source::SourceFrameRequestImage {
                    request_kind: runtime_source::SourceFrameRequestKind::Message,
                    field: admission::admission_discontinuous_empty_child_field(),
                    child: live_runtime_discontinuous_send_child(),
                },
                live_runtime_send_channel_storage(frame_bytes),
            ).unwrap(),
            runtime_source::SourceFrameRequestImage {
                request_kind: runtime_source::SourceFrameRequestKind::Message,
                field: admission::admission_discontinuous_empty_child_field(),
                child: live_runtime_discontinuous_send_child(),
            },
            live_runtime_send_channel_storage(frame_bytes),
        ),
        live_runtime_product_wf(live_runtime_indexed_message_push_after(
            live_runtime_discontinuous_send_initial(frame_bytes).unwrap(),
            runtime_source::SourceFrameRequestImage {
                request_kind: runtime_source::SourceFrameRequestKind::Message,
                field: admission::admission_discontinuous_empty_child_field(),
                child: live_runtime_discontinuous_send_child(),
            },
            live_runtime_send_channel_storage(frame_bytes),
        ).unwrap()),
        product_indexed_start_anchor(
            live_runtime_indexed_message_push_after(
                live_runtime_discontinuous_send_initial(frame_bytes).unwrap(),
                runtime_source::SourceFrameRequestImage {
                    request_kind: runtime_source::SourceFrameRequestKind::Message,
                    field: admission::admission_discontinuous_empty_child_field(),
                    child: live_runtime_discontinuous_send_child(),
                },
                live_runtime_send_channel_storage(frame_bytes),
            ).unwrap().source.product.indexed_regions.last(),
        ) == (epm::SourceAnchor { source: 1, offset: 100 }),
        live_runtime_indexed_message_push_after(
            live_runtime_discontinuous_send_initial(frame_bytes).unwrap(),
            runtime_source::SourceFrameRequestImage {
                request_kind: runtime_source::SourceFrameRequestKind::Message,
                field: admission::admission_discontinuous_empty_child_field(),
                child: live_runtime_discontinuous_send_child(),
            },
            live_runtime_send_channel_storage(frame_bytes),
        ).unwrap().source.product.bridge.admission.frames.last().physical_source == 1,
{
    live_runtime_discontinuous_send_initial_is_wf(frame_bytes);
    live_runtime_discontinuous_send_admission_push_is_possible(frame_bytes);
    live_runtime_discontinuous_send_wire_field_is_read(frame_bytes);
    let before = live_runtime_discontinuous_send_initial(frame_bytes).unwrap();
    let field = admission::admission_discontinuous_empty_child_field();
    let child = live_runtime_discontinuous_send_child();
    let request = runtime_source::SourceFrameRequestImage {
        request_kind: runtime_source::SourceFrameRequestKind::Message,
        field, child,
    };
    let storage = live_runtime_send_channel_storage(frame_bytes);
    let parent = before.cursors[0].unwrap();
    let parent_source = product_admission_source_image(
        before.source.product.indexed_regions[0],
    );
    assert(runtime_source::source_successful_push_storage(
        before.source.runtime, storage,
    ));
    assert(runtime_source::source_checked_frame_request_push_indexed(
        before.source.runtime, request, parent_source, storage,
    ).is_some());
    assert(product_indexed_child(
        before.source.product.indexed_regions[0], field,
    ).is_some());
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field: observed, start,
    }) = wire_source::wire_runtime_next_field(parent) {
        assert(observed == field);
        live_runtime_child_cursor_is_payload(parent, concrete_after, observed, start);
        live_runtime_parent_cursor_after_field_projects(
            parent, concrete_after, observed, start,
        );
        assert(live_runtime_child_cursor(parent, field).is_some());
        let child_cursor = live_runtime_child_cursor(parent, field).unwrap();
        assert(wire_source::wire_runtime_cursor_project(child_cursor).is_some());
        let parent_next = runtime_wire_after_field(parent, concrete_after);
        assert(wire_source::wire_runtime_cursor_project(parent_next).is_some());
        assert(live_runtime_indexed_message_push_after(
            before, request, storage,
        ).is_some());
        let after = live_runtime_indexed_message_push_after(
            before, request, storage,
        ).unwrap();
        assert(before.source.product.accepted_epm_sources == seq![None]);
        assert(after.source.product.accepted_epm_sources == seq![None, None]);
        assert forall|i: int| 0 <= i < 2 implies
            product_epm_receipts_carried(
                before.source.product, after.source.product,
            )[i] == None by {}
        assert(after.source.product.accepted_epm_sources
            == product_epm_receipts_carried(
                before.source.product, after.source.product,
            ));
        assert(live_runtime_product_message_push_indexed_step(
            before, after, request, storage,
        ));
        live_runtime_product_message_push_indexed_preserves_wf(
            before, after, request, storage,
        );
        assert(product_indexed_start_anchor(
            after.source.product.indexed_regions.last(),
        ) == epm::SourceAnchor { source: 1, offset: 100 });
    }
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_discontinuous_send_indexed_after(
    frame_bytes: usize,
) -> Option<LiveRuntimeProductState> {
    match live_runtime_discontinuous_send_initial(frame_bytes) {
        None => None,
        Some(before) => live_runtime_indexed_message_push_after(
            before,
            runtime_source::SourceFrameRequestImage {
                request_kind: runtime_source::SourceFrameRequestKind::Message,
                field: admission::admission_discontinuous_empty_child_field(),
                child: live_runtime_discontinuous_send_child(),
            },
            live_runtime_send_channel_storage(frame_bytes),
        ),
    }
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_discontinuous_send_indexed_observed_push_is_reachable(
    frame_bytes: usize,
)
    requires
        frame_bytes > 0,
        2 * frame_bytes as int <= usize::MAX as int,
        2 <= usize::MAX as int / frame_bytes as int,
    ensures
        live_runtime_discontinuous_send_indexed_after(frame_bytes).is_some(),
        live_runtime_product_wf(
            live_runtime_discontinuous_send_indexed_after(frame_bytes).unwrap(),
        ),
        product_indexed_start_anchor(
            live_runtime_discontinuous_send_indexed_after(frame_bytes).unwrap()
                .source.product.indexed_regions.last(),
        ) == (epm::SourceAnchor { source: 1, offset: 100 }),
        live_runtime_discontinuous_send_indexed_after(frame_bytes).unwrap()
            .source.product.bridge.admission.frames.last().physical_source == 1,
        product_wire_indexed_source_agrees(
            live_runtime_discontinuous_send_indexed_after(frame_bytes).unwrap()
                .cursors.last().unwrap(),
            live_runtime_discontinuous_send_indexed_after(frame_bytes).unwrap()
                .source.product.indexed_regions.last(),
        ),
        exists|request: runtime_source::SourceFrameRequestImage|
            live_runtime_product_message_push_indexed_observed_step(
                live_runtime_discontinuous_send_initial(frame_bytes).unwrap(),
                live_runtime_discontinuous_send_indexed_after(frame_bytes).unwrap(),
                request,
                Some(runtime_source::SourceFrameRequestKind::Message),
                2, 2, frame_bytes,
            ),
{
    live_runtime_discontinuous_send_indexed_push_reaches_live_state(frame_bytes);
    let before = live_runtime_discontinuous_send_initial(frame_bytes).unwrap();
    let after = live_runtime_discontinuous_send_indexed_after(frame_bytes).unwrap();
    let request = runtime_source::SourceFrameRequestImage {
        request_kind: runtime_source::SourceFrameRequestKind::Message,
        field: admission::admission_discontinuous_empty_child_field(),
        child: live_runtime_discontinuous_send_child(),
    };
    runtime_source::source_frame_push_observe_after_exact(
        runtime_source::SourceFrameRequestKind::Message,
        Some(runtime_source::SourceFrameRequestKind::Message),
        1, 1, 2, 2, frame_bytes, frame_bytes,
    );
    assert(runtime_source::source_frame_push_observe_after(
        request.request_kind,
        Some(runtime_source::SourceFrameRequestKind::Message),
        1, 1, 2, 2, frame_bytes, frame_bytes,
    ) == runtime_source::SourceFramePushObservationResult::Success {
        storage: live_runtime_send_channel_storage(frame_bytes),
    });
    assert(live_runtime_product_message_push_indexed_observed_step(
        before, after, request,
        Some(runtime_source::SourceFrameRequestKind::Message),
        2, 2, frame_bytes,
    ));
    live_runtime_product_message_push_indexed_source_ids_match(
        before, after, request, live_runtime_send_channel_storage(frame_bytes),
    );
    live_runtime_product_message_push_indexed_observed_is_exact(
        before, after, request,
        Some(runtime_source::SourceFrameRequestKind::Message),
        2, 2, frame_bytes,
    );
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_send_channel_initial_is_wf(frame_bytes: usize)
    requires frame_bytes > 0,
    ensures
        live_runtime_send_channel_initial(frame_bytes).is_some(),
        live_runtime_product_wf(live_runtime_send_channel_initial(frame_bytes).unwrap()),
{
    let root = live_runtime_send_channel_root();
    let indexed_backing = epm::IndexedBackingImage::Borrowed {
        bytes: root.source_bytes,
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 2,
        end_override: None,
    };
    assert(admission::admission_root_initial(root));
    map::integrated_initial_is_well_formed(root);
    assert(epm::indexed_backing_project(indexed_backing).is_some());
    let indexed = epm::indexed_backing_project(indexed_backing).unwrap();
    assert(epm::indexed_image_wf(indexed));
    assert(indexed.bytes =~= root.source_bytes);
    assert(product_root_initial(root, indexed_backing).is_some());
    product_root_initial_exact(root, indexed_backing);
    admission::admission_initial_wf(root);
    score_bridge::bridge_initial_exact(root);
    let state = live_runtime_send_channel_initial(frame_bytes).unwrap();
    assert(state.source.runtime.score == state.source.product.bridge);
    assert(runtime_source::runtime_frame_image_wf(state.source.runtime));
    assert(arena_source::runtime_projects_score(
        state.source.arena, state.source.product.bridge.transcript,
    ));
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_send_channel_child() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: 0,
        physical_start: 2,
        program_type_id: 1,
        region_start: 0,
        region_end: 0,
        source_bytes: Seq::empty(),
        cursor: 0,
        slot_count: 12,
        last_slot: None,
        seen: Set::empty(),
        required: Set::empty(),
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        family: admission::AdmissionFamily::Other,
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

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_send_channel_child_is_valid()
    ensures
        admission::admission_frame_wf(live_runtime_send_channel_child()),
        admission::admission_child_physical_contained(
            live_runtime_send_channel_root(), live_runtime_send_channel_child(),
        ),
{
    assert(live_runtime_send_channel_root().source_bytes.subrange(2, 2)
        =~= Seq::<u8>::empty());
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_send_channel_storage(
    frame_bytes: usize,
) -> runtime_source::SourceFramePushStorageImage {
    runtime_source::SourceFramePushStorageImage {
        request_kind: runtime_source::SourceFrameRequestKind::Message,
        before_len: 1,
        after_len: 2,
        before_capacity: 1,
        after_capacity: 2,
        frame_variant_bytes: frame_bytes,
        stats_frame_variant_bytes: frame_bytes,
        used_bytes: (2 * frame_bytes as int) as usize,
        reserved_bytes: (2 * frame_bytes as int) as usize,
    }
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_send_channel_push_is_possible(frame_bytes: usize)
    requires
        frame_bytes > 0,
        2 * frame_bytes as int <= usize::MAX as int,
        2 <= usize::MAX as int / frame_bytes as int,
    ensures match live_runtime_send_channel_initial(frame_bytes) {
        None => false,
        Some(before) => match before.cursors.last() {
            None => false,
            Some(cursor) => match wire_source::wire_runtime_next_field(cursor) {
                Some(wire_source::WireConcreteTransition::Field {
                    field, ..
                }) => {
                    let child = live_runtime_send_channel_child();
                    let request = runtime_source::SourceFrameRequestImage {
                        request_kind: runtime_source::SourceFrameRequestKind::Message,
                        field,
                        child,
                    };
                    &&& match map::integrated_admission_driver_step(
                        before.source.product.map,
                        map::IntegratedAdmissionAction::Message {
                            action: admission::ConcreteFrameActionModel::Push {
                                field, child, frame_bytes,
                            },
                        },
                    ) {
                        Some(map::IntegratedAdmissionOutcome::MessagePushed { .. }) => true,
                        _ => false,
                    }
                    &&& runtime_source::source_checked_frame_request_push(
                        before.source.runtime, request,
                        live_runtime_send_channel_storage(frame_bytes),
                    ).is_some()
                },
                _ => false,
            },
        },
    },
{
    live_runtime_send_channel_initial_is_wf(frame_bytes);
    live_runtime_send_channel_child_is_valid();
    wire_source::wire_empty_par_field_is_read();
    let before = live_runtime_send_channel_initial(frame_bytes).unwrap();
    let cursor = before.cursors.last().unwrap();
    assert(cursor == wire_source::wire_runtime_initial(
        wire_source::wire_empty_par_field_backing(), false,
    ).unwrap());
    let derived = wire_source::wire_runtime_derive_cursor(cursor).unwrap();
    assert(wire_source::wire_concrete_cursor_wf(derived));
    wire_source::wire_runtime_next_field_from_source(cursor);
    if let Some(wire_source::WireConcreteTransition::Field {
        field, ..
    }) = wire_source::wire_runtime_next_field(cursor) {
        let source = wire_source::wire_runtime_cursor_project(cursor).unwrap();
        wire_source::wire_source_next_field_refines(source);
        assert(admission::indexed_field_at_cursor(source.cursor, field));
        let child = live_runtime_send_channel_child();
        let request = runtime_source::SourceFrameRequestImage {
            request_kind: runtime_source::SourceFrameRequestKind::Message,
            field,
            child,
        };
        assert(admission::concrete_message_view(
            before.source.product.bridge.admission,
        ).is_some());
        assert(before.source.product.bridge.admission.frames[0]
            == live_runtime_send_channel_root());
        assert(field.encoded_end == 2);
        assert(before.source.product.bridge.admission.frames[0].cursor == 0);
        assert(admission::admission_child_physical_contained(
            before.source.product.bridge.admission.frames[0], child,
        ));
        assert(child.physical_start as int
            == before.source.product.bridge.admission.frames[0].physical_start as int
                + field.payload_start as int);
        assert(child.region_end as int
            == field.payload_end as int - field.payload_start as int);
        assert(admission::admission_push_step(
            before.source.product.bridge.admission,
            field.encoded_end, child, 0, false, field.number, frame_bytes,
        ).is_some());
        assert(admission::concrete_message_push(
            admission::concrete_message_view(
                before.source.product.bridge.admission,
            ).unwrap(),
            field, child, frame_bytes,
        ).is_some());
        assert(admission::concrete_message_driver_step(
            before.source.product.bridge.admission,
            admission::ConcreteFrameActionModel::Push {
                field, child, frame_bytes,
            },
        ).is_some());
        assert(map::integrated_admission_driver_step(
            before.source.product.map,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Push {
                    field, child, frame_bytes,
                },
            },
        ).is_some());
        assert(runtime_source::source_checked_frame_request_push(
            before.source.runtime, request,
            live_runtime_send_channel_storage(frame_bytes),
        ).is_some());
    }
}

#[cfg(rpb3_live_witness)]
pub open spec fn live_runtime_send_channel_push_after(
    frame_bytes: usize,
) -> Option<LiveRuntimeProductState> {
    match live_runtime_send_channel_initial(frame_bytes) {
        None => None,
        Some(before) => match before.cursors.last() {
            None => None,
            Some(parent) => match wire_source::wire_runtime_next_field(parent) {
                Some(wire_source::WireConcreteTransition::Field {
                    state: concrete_after, field, start: _,
                }) => {
                    let child = live_runtime_send_channel_child();
                    let request = runtime_source::SourceFrameRequestImage {
                        request_kind: runtime_source::SourceFrameRequestKind::Message,
                        field, child,
                    };
                    let action = map::IntegratedAdmissionAction::Message {
                        action: admission::ConcreteFrameActionModel::Push {
                            field, child, frame_bytes,
                        },
                    };
                    match (
                        map::integrated_admission_driver_step(
                            before.source.product.map, action,
                        ),
                        product_indexed_child(
                            before.source.product.indexed_regions.last(), field,
                        ),
                        runtime_source::source_checked_frame_request_push(
                            before.source.runtime, request,
                            live_runtime_send_channel_storage(frame_bytes),
                        ),
                        live_runtime_child_cursor(parent, field),
                    ) {
                        (
                            Some(map::IntegratedAdmissionOutcome::MessagePushed {
                                state: next_map,
                            }),
                            Some(child_region),
                            Some(next_runtime),
                            Some(child_cursor),
                        ) => {
                            let next_parent = runtime_wire_after_field(
                                parent, concrete_after,
                            );
                            match (
                                wire_source::wire_runtime_cursor_project(next_parent),
                                wire_source::wire_runtime_cursor_project(child_cursor),
                            ) {
                                (Some(next_parent_reader), Some(child_reader)) => {
                                    let product = ProductScoreState {
                                        bridge: score_bridge::ScoreAdmissionBridge {
                                            admission: next_map.admission,
                                            transcript: next_map.transcript,
                                        },
                                        map: next_map,
                                        epm_frames: before.source.product.epm_frames.push(None),
                                        indexed_regions: before.source.product.indexed_regions.push(
                                            child_region,
                                        ),
                                        accepted_epm_sources:
                                            before.source.product.accepted_epm_sources.push(None),
                                    };
                                    Some(LiveRuntimeProductState {
                                        source: LiveSourceProductState {
                                            product,
                                            runtime: next_runtime,
                                            arena: before.source.arena,
                                            wire: before.source.wire.update(
                                                before.source.wire.len() - 1,
                                                Some(next_parent_reader),
                                            ).push(Some(child_reader)),
                                        },
                                        cursors: before.cursors.update(
                                            before.cursors.len() - 1, Some(next_parent),
                                        ).push(Some(child_cursor)),
                                    })
                                },
                                _ => None,
                            }
                        },
                        _ => None,
                    }
                },
                _ => None,
            },
        },
    }
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_send_channel_push_reaches_live_state(frame_bytes: usize)
    requires
        frame_bytes > 0,
        2 * frame_bytes as int <= usize::MAX as int,
        2 <= usize::MAX as int / frame_bytes as int,
    ensures
        live_runtime_send_channel_push_after(frame_bytes).is_some(),
        live_runtime_product_wf(live_runtime_send_channel_push_after(frame_bytes).unwrap()),
        exists|request: runtime_source::SourceFrameRequestImage|
            live_runtime_product_message_push_step(
                live_runtime_send_channel_initial(frame_bytes).unwrap(),
                live_runtime_send_channel_push_after(frame_bytes).unwrap(),
                request,
                live_runtime_send_channel_storage(frame_bytes),
            ),
        exists|request: runtime_source::SourceFrameRequestImage|
            live_runtime_product_message_push_observed_step(
                live_runtime_send_channel_initial(frame_bytes).unwrap(),
                live_runtime_send_channel_push_after(frame_bytes).unwrap(),
                request,
                Some(runtime_source::SourceFrameRequestKind::Message),
                2, 2, frame_bytes,
            ),
{
    live_runtime_send_channel_push_is_possible(frame_bytes);
    let before = live_runtime_send_channel_initial(frame_bytes).unwrap();
    let parent = before.cursors.last().unwrap();
    if let Some(wire_source::WireConcreteTransition::Field {
        state: concrete_after, field, start,
    }) = wire_source::wire_runtime_next_field(parent) {
        live_runtime_child_cursor_is_payload(parent, concrete_after, field, start);
        live_runtime_parent_cursor_after_field_projects(
            parent, concrete_after, field, start,
        );
        assert(product_indexed_child(
            before.source.product.indexed_regions.last(), field,
        ).is_some());
        let request = runtime_source::SourceFrameRequestImage {
            request_kind: runtime_source::SourceFrameRequestKind::Message,
            field,
            child: live_runtime_send_channel_child(),
        };
        let after = live_runtime_send_channel_push_after(frame_bytes).unwrap();
        assert(before.source.product.accepted_epm_sources == seq![None]);
        assert(after.source.product.accepted_epm_sources == seq![None, None]);
        assert(product_epm_receipts_carried(
            before.source.product, after.source.product,
        ).len() == 2);
        assert forall|i: int| 0 <= i < 2 implies
            product_epm_receipts_carried(
                before.source.product, after.source.product,
            )[i] == None by {
            if i == 0 {
                assert(before.source.product.accepted_epm_sources[i] == None);
            }
        }
        assert(after.source.product.accepted_epm_sources
            == product_epm_receipts_carried(
                before.source.product, after.source.product,
            ));
        assert(live_runtime_product_message_push_step(
            before,
            after,
            request,
            live_runtime_send_channel_storage(frame_bytes),
        ));
        assert(before.source.product.bridge.admission.frames.len() == 1);
        assert(before.source.runtime.capacity == 1);
        runtime_source::source_frame_push_observe_after_exact(
            runtime_source::SourceFrameRequestKind::Message,
            Some(runtime_source::SourceFrameRequestKind::Message),
            1, 1, 2, 2, frame_bytes, frame_bytes,
        );
        assert(runtime_source::source_frame_push_observe_after(
            runtime_source::SourceFrameRequestKind::Message,
            Some(runtime_source::SourceFrameRequestKind::Message),
            1, 1, 2, 2, frame_bytes, frame_bytes,
        ) == runtime_source::SourceFramePushObservationResult::Success {
            storage: live_runtime_send_channel_storage(frame_bytes),
        });
        assert(live_runtime_product_message_push_observed_step(
            before,
            live_runtime_send_channel_push_after(frame_bytes).unwrap(),
            request,
            Some(runtime_source::SourceFrameRequestKind::Message),
            2, 2, frame_bytes,
        ));
        live_runtime_product_message_push_observed_is_exact(
            before,
            live_runtime_send_channel_push_after(frame_bytes).unwrap(),
            request,
            Some(runtime_source::SourceFrameRequestKind::Message),
            2, 2, frame_bytes,
        );
        live_runtime_product_message_push_preserves_wf(
            before,
            live_runtime_send_channel_push_after(frame_bytes).unwrap(),
            request,
            live_runtime_send_channel_storage(frame_bytes),
        );
    }
}

#[cfg(rpb3_live_witness)]
pub proof fn live_runtime_send_channel_child_end_is_exact(frame_bytes: usize)
    requires
        frame_bytes > 0,
        2 * frame_bytes as int <= usize::MAX as int,
        2 <= usize::MAX as int / frame_bytes as int,
    ensures
        live_runtime_send_channel_push_after(frame_bytes).is_some(),
        wire_source::wire_runtime_next_field(
            live_runtime_send_channel_push_after(frame_bytes).unwrap().cursors[1].unwrap(),
        ) == Some(wire_source::WireConcreteTransition::End {
            state: wire_source::wire_runtime_derive_cursor(
                live_runtime_send_channel_push_after(frame_bytes).unwrap().cursors[1].unwrap(),
            ).unwrap(),
        }),
        live_runtime_send_channel_push_after(frame_bytes).unwrap()
            .source.product.bridge.admission.frames[1].cursor
            == live_runtime_send_channel_push_after(frame_bytes).unwrap()
                .source.product.bridge.admission.frames[1].region_end,
{
    live_runtime_send_channel_push_reaches_live_state(frame_bytes);
    let state = live_runtime_send_channel_push_after(frame_bytes).unwrap();
    let child = state.cursors[1].unwrap();
    assert(wire_source::wire_runtime_next_field(child)
        == Some(wire_source::WireConcreteTransition::End {
            state: wire_source::wire_runtime_derive_cursor(child).unwrap(),
        }));
    wire_source::wire_runtime_end_is_exact(child);
    assert(state.source.wire[1] == wire_source::wire_runtime_cursor_project(child));
}

}

fn main() {}
