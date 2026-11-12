//! Open obligation 4 of `docs/design/pathmap/rpb3-admission-source-traceability.md`: the
//! top-handle body-origin, semantic-flag and transcript facts that the checked root theorem
//! (`bridge_root_exact`, lifted by the root module's `live_runtime_root_complete_matches_source`)
//! states for a completed root hold for every successful selected finish, including child returns.
//!
//! `finish_handle_facts(bridge, frame, h)` is `bridge_root_exact`'s conclusion for any frame:
//! `h` is in range of the admission arena and of the score transcript, its binding originates from
//! the frame's body (`admission_score_origin_matches`), its semantic flag is the frame's family
//! completeness, and its bytes are the checked transcript entry `h` (`entry_well_formed`,
//! `handle_bound_to_bytes`). `finish_top_handle(after)` states these facts for the score handle of
//! the finished top frame.
//!
//! Every finish kind. Each generic score candidate folded by `bridge_score_finish_selected` ends in
//! `admission_attach_score_step`, whose guard contains the origin and semantic-flag agreement
//! (`attach_agreement_holds`), and the candidate's `_exact` lemma gives the checked bridge that
//! carries the transcript facts. One lemma per candidate (`handle_after_*`, the 43 candidates of
//! `rpb3_score_finish_groups` in fold order) proves the facts for that finish; six group lemmas
//! (`handle_routed_group_K`) dispatch on the route of `rpb3_score_finish_groups`; and with
//! `bridge_score_finish_never_ambiguous` of `rpb3_score_finish_unique` the selected finish is the
//! routed candidate (`handle_after_selected_finish`). The EPathMap source finish selected on an
//! `EofEPathMap` event has its own lemma (`handle_after_epm_source_finish`), and so has its
//! product-level form at the root (`handle_after_product_epm_finish`).
//!
//! Child returns. `admission_complete_child_step` accepts a child only if its summary agrees with
//! it (`admission_child_summary_agrees`), so the parent's new return carries the child's score
//! handle with the same facts and an origin body id read from the arena, or no handle and no origin
//! (`child_return_carries_handle`); every runtime child pop goes through it
//! (`pre_pop_return_carries_handle`).
//!
//! Runtime outcomes. Every outcome of `runtime_message_driver_step` on `Eof` (scored or unscored
//! child return, scored or unscored root return) and on `EofEPathMap` (child or root return) states
//! the facts of its finish (`runtime_eof_finish_facts`, `runtime_epathmap_eof_finish_facts`); the
//! live root-score, child-score and root-complete steps inherit them, and the live EPathMap EOF
//! (`live_runtime_epm_source_eof_selected`) is the runtime `EofEPathMap` step.
//! `handle_owner_witness` is a concrete selected child finish: the modeled EPathMap owner at its
//! EOF.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_runtime_frame_source_refinement as runtime_source,
    rpb3_score_admission_bridge as bridge, rpb3_score_finish_groups as groups,
    rpb3_score_finish_unique as unique, rpb3_score_transcript as score,
};

verus! {

/// The checked root theorem's conclusion for the score handle `handle` of `frame`: body origin,
/// semantic flag and transcript.
pub open spec fn finish_handle_facts(
    bridge: bridge::ScoreAdmissionBridge, frame: admission::AdmissionFrameState, handle: usize,
) -> bool {
    &&& (handle as int) < bridge.admission.arena.len()
    &&& (handle as int) < bridge.transcript.entries.len()
    &&& admission::admission_score_origin_matches(frame, bridge.admission.arena, handle)
    &&& bridge.admission.arena[handle as int].semantic
        == admission::admission_family_complete(frame)
    &&& bridge.admission.arena[handle as int].encoded_len
        == bridge.transcript.entries[handle as int].encoded_len
    &&& bridge.admission.arena[handle as int].transcript_bytes
        == bridge.transcript.entries[handle as int].transcript
    &&& score::entry_well_formed(bridge.transcript.entries, handle as int)
    &&& score::handle_bound_to_bytes(
        bridge.transcript, handle, bridge.transcript.entries[handle as int].transcript,
    )
}

/// The finished top frame carries a score handle with the top-handle facts. Opaque, so that the
/// finish-kind dispatch carries one atom per kind.
#[verifier::opaque]
pub open spec fn finish_top_handle(after: bridge::ScoreAdmissionBridge) -> bool {
    &&& after.admission.frames.len() > 0
    &&& match after.admission.frames.last().score_handle {
        None => false,
        Some(handle) => finish_handle_facts(after, after.admission.frames.last(), handle),
    }
}

/// A child return as the parent sees it after the pop: its last return carries the child's score
/// handle, body id and body bytes; without a handle it has no origin, with a handle `h` its origin
/// is the arena binding's body id and `h` keeps the top-handle facts of the child.
pub open spec fn finish_child_return_facts(
    child: admission::AdmissionFrameState, popped: bridge::ScoreAdmissionBridge,
) -> bool {
    let returned = popped.admission.frames.last().returns.last();
    &&& popped.admission.frames.len() > 0
    &&& popped.admission.frames.last().returns.len() > 0
    &&& returned.score_handle == child.score_handle
    &&& returned.body_id == child.body_id
    &&& returned.body_bytes == child.source_bytes
    &&& match child.score_handle {
        None => returned.score_origin_body_id.is_none(),
        Some(handle) => {
            &&& returned.score_origin_body_id
                == Some(popped.admission.arena[handle as int].body_id)
            &&& finish_handle_facts(popped, child, handle)
        },
    }
}

/// A root return: the one pre-pop frame's score handle is the summary's, the returned score bytes
/// are its checked transcript entry, and the handle has the top-handle facts.
pub open spec fn finish_root_return_facts(
    returned: runtime_source::RuntimeRootReturnImage,
) -> bool {
    let scored = returned.pre_pop.score;
    let handle = returned.summary.score_handle;
    &&& returned.remaining_frames == 0
    &&& scored.admission.frames.len() == 1
    &&& scored.admission.frames[0].score_handle == Some(handle)
    &&& returned.score_bytes == scored.transcript.entries[handle as int].transcript
    &&& finish_handle_facts(scored, scored.admission.frames[0], handle)
}

/// Origin and semantic-flag agreement plus a checked bridge give the top-handle facts (the body of
/// `bridge_root_exact`, for any frame).
pub proof fn handle_facts_from_agreement(
    bridge: bridge::ScoreAdmissionBridge, frame: admission::AdmissionFrameState,
)
    requires
        bridge::bridge_well_formed(bridge),
        admission::admission_score_body_agrees(frame, bridge.admission.arena),
    ensures
        frame.score_handle.is_some(),
        finish_handle_facts(bridge, frame, frame.score_handle.unwrap()),
{
    let handle = frame.score_handle.unwrap();
    assert(score::entry_well_formed(bridge.transcript.entries, handle as int));
    let bytes = bridge.transcript.entries[handle as int].transcript;
    assert(bytes.len() == bridge.transcript.entries[handle as int].encoded_len as int);
    assert(bytes.len() <= usize::MAX as int);
    assert(score::handle_transcript(bridge.transcript, handle) == Some(bytes));
    assert(score::handle_encoded_len(bridge.transcript, handle) == Some(bytes.len() as usize));
}

pub proof fn top_handle_from_agreement(after: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(after),
        after.admission.frames.len() > 0,
        admission::admission_score_body_agrees(
            after.admission.frames.last(), after.admission.arena,
        ),
    ensures finish_top_handle(after),
{
    reveal(finish_top_handle);
    handle_facts_from_agreement(after, after.admission.frames.last());
}

/// The attach guard contains the agreement: a successful attach leaves a top frame whose score
/// handle agrees with its body origin and semantic flag, with no precondition on the state.
pub proof fn attach_agreement_holds(
    state: admission::AdmissionState, handle: usize, body_id: u64,
)
    ensures match admission::admission_attach_score_step(state, handle, body_id) {
        None => true,
        Some(next) => {
            &&& next.frames.len() == state.frames.len()
            &&& next.frames.len() > 0
            &&& next.arena == state.arena
            &&& next.frames.last().score_handle == Some(handle)
            &&& admission::admission_score_body_agrees(next.frames.last(), next.arena)
        },
    },
{
    if let Some(next) = admission::admission_attach_score_step(state, handle, body_id) {
        let index = state.frames.len() - 1;
        let before = state.frames[index];
        let after = next.frames[index];
        assert(next.frames.len() == state.frames.len());
        assert(after == next.frames.last());
        assert(next.arena == state.arena);
        assert(after.score_handle == Some(handle));
        assert(after.body_id == before.body_id);
        assert(after.program_type_id == before.program_type_id);
        assert(after.returns == before.returns);
        assert(after.family == before.family);
        assert(after.seen == before.seen);
        assert(after.required == before.required);
        assert(after.scalar_slots == before.scalar_slots);
        assert(admission::admission_family_complete(after)
            == admission::admission_family_complete(before));
        assert(admission::admission_score_origin_matches(after, next.arena, handle));
        assert(admission::admission_score_body_agrees(after, next.arena));
    }
}

/// `attach_agreement_holds` for every attach term, so that it applies inside an unfolded finish.
pub proof fn attach_agreement_everywhere()
    ensures
        forall|state: admission::AdmissionState, handle: usize, body_id: u64|
            #![trigger admission::admission_attach_score_step(state, handle, body_id)]
            admission::admission_attach_score_step(state, handle, body_id).is_some() ==> {
                let next = admission::admission_attach_score_step(state, handle, body_id).unwrap();
                &&& next.frames.len() > 0
                &&& next.arena == state.arena
                &&& admission::admission_score_body_agrees(next.frames.last(), next.arena)
            },
{
    assert forall|state: admission::AdmissionState, handle: usize, body_id: u64|
        #![trigger admission::admission_attach_score_step(state, handle, body_id)]
        admission::admission_attach_score_step(state, handle, body_id).is_some() implies {
            let next = admission::admission_attach_score_step(state, handle, body_id).unwrap();
            &&& next.frames.len() > 0
            &&& next.arena == state.arena
            &&& admission::admission_score_body_agrees(next.frames.last(), next.arena)
        } by {
        attach_agreement_holds(state, handle, body_id);
    }
}

pub proof fn handle_after_raw_par(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_raw_par(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_raw_par);
    bridge::bridge_raw_par_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_raw_par(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_cost_atom_absent(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_cost_atom_absent(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_cost_atom_absent);
    bridge::bridge_cost_atom_absent_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_cost_atom_absent(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_cost_atom_bytes(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_cost_atom_bytes(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_cost_atom_bytes);
    bridge::bridge_cost_atom_bytes_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_cost_atom_bytes(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_cost_bound(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_cost_bound(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_cost_bound);
    bridge::bridge_cost_bound_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_cost_bound(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_cost_atom_bound_forward(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_cost_atom_bound_forward(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_cost_atom_bound_forward);
    bridge::bridge_cost_atom_bound_forward_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_cost_atom_bound_forward(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_cost_signature(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_cost_signature(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_cost_signature);
    bridge::bridge_cost_signature_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_cost_signature(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_cost_region(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_cost_region(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_cost_region);
    bridge::bridge_cost_region_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_cost_region(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_cost_token_stack(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_cost_token_stack(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_cost_token_stack);
    bridge::bridge_cost_token_stack_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_cost_token_stack(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_bundle(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_bundle(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_bundle);
    bridge::bridge_bundle_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_bundle(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_if(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_if(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_if);
    bridge::bridge_if_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_if(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_send(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_send(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_send);
    bridge::bridge_send_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_send(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_match_component(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_match_component(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_match_component);
    bridge::bridge_match_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_match_component(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_absent_specialized(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_absent_specialized(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_absent_specialized);
    bridge::bridge_absent_specialized_constructor_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_absent_specialized(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_absent_connective(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_absent_connective(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_absent_connective);
    bridge::bridge_absent_connective_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_absent_connective(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_receive_bind(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_receive_bind(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_receive_bind);
    bridge::bridge_receive_bind_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_receive_bind(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_match_case(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_match_case(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_match_case);
    bridge::bridge_match_case_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_match_case(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_bool_oneof(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_bool_oneof(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_bool_oneof);
    bridge::bridge_bool_oneof_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_bool_oneof(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_expr_atom(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_expr_atom(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_expr_atom);
    bridge::bridge_expr_atom_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_expr_atom(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_expr_numeric(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_expr_numeric(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_expr_numeric);
    bridge::bridge_expr_numeric_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_expr_numeric(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_expr_big_rational(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_expr_big_rational(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_expr_big_rational);
    bridge::bridge_expr_big_rational_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_expr_big_rational(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_expr_fixed_point(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_expr_fixed_point(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_expr_fixed_point);
    bridge::bridge_expr_fixed_point_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_expr_fixed_point(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_sys_auth_token(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_sys_auth_token(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_sys_auth_token);
    bridge::bridge_sys_auth_token_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_sys_auth_token(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_new_empty_collections(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_new_empty_collections(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_new_empty_collections);
    bridge::bridge_new_empty_collections_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_new_empty_collections(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_new_one_injection(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_new_one_injection(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_new_one_injection);
    bridge::bridge_new_one_injection_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_new_one_injection(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_new_one_absent_injection(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_new_one_absent_injection(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_new_one_absent_injection);
    bridge::bridge_new_one_absent_injection_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_new_one_absent_injection(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_new_one_uri(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_new_one_uri(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_new_one_uri);
    bridge::bridge_new_one_uri_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_new_one_uri(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_new_collections(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_new_collections(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_new_collections);
    bridge::bridge_new_collections_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_new_collections(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_expr_payload_forward(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_expr_payload_forward(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_expr_payload_forward);
    bridge::bridge_expr_payload_forward_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_expr_payload_forward(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_expr_structural(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_expr_structural(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_expr_structural);
    bridge::bridge_expr_structural_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_expr_structural(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_evar(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_evar(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_evar);
    bridge::bridge_evar_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_evar(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_ordered_sequence(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_ordered_sequence(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_ordered_sequence);
    bridge::bridge_ordered_sequence_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_ordered_sequence(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_emethod(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_emethod(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_emethod);
    bridge::bridge_emethod_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_emethod(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_emap_finish(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_emap_finish(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_emap_finish);
    bridge::bridge_emap_complete_finish_source_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_emap_finish(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_connective_not(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_connective_not(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_connective_not);
    bridge::bridge_connective_not_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_connective_not(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_connective_varref(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_connective_varref(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_connective_varref);
    bridge::bridge_connective_varref_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_connective_varref(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_connective_collection(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_connective_collection(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_connective_collection);
    bridge::bridge_connective_collection_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_connective_collection(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_unforgeable_bytes(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_unforgeable_bytes(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_unforgeable_bytes);
    bridge::bridge_unforgeable_bytes_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_unforgeable_bytes(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_receive_present_condition(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_receive_present_condition(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_receive_present_condition);
    bridge::bridge_receive_present_condition_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_receive_present_condition(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_receive_absent_condition(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_receive_absent_condition(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_receive_absent_condition);
    bridge::bridge_receive_absent_condition_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_receive_absent_condition(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_nonsemantic(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_nonsemantic(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_nonsemantic);
    bridge::bridge_nonsemantic_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_nonsemantic(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_var(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_var(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_var);
    bridge::bridge_var_score_step_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_var(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_epathmap_owner(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_epathmap_owner(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_epathmap_owner);
    bridge::bridge_epathmap_owner_score_step_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_epathmap_owner(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_after_ezipper(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_candidate_ezipper(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_candidate_ezipper);
    bridge::bridge_ezipper_score_step_exact(before);
    attach_agreement_everywhere();
    if let Some(after) = groups::finish_candidate_ezipper(before) {
        top_handle_from_agreement(after);
    }
}

pub proof fn handle_routed_group_1(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_routed_group_1(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    let route = groups::finish_route_of(before);
    if route == 1 {
        handle_after_raw_par(before);
    } else if route == 2 {
        handle_after_cost_atom_absent(before);
    } else if route == 3 {
        handle_after_cost_atom_bytes(before);
    } else if route == 4 {
        handle_after_cost_bound(before);
    } else if route == 5 {
        handle_after_cost_atom_bound_forward(before);
    } else if route == 6 {
        handle_after_cost_signature(before);
    } else if route == 7 {
        handle_after_cost_region(before);
    } else if route == 8 {
        handle_after_cost_token_stack(before);
    }
}

pub proof fn handle_routed_group_2(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_routed_group_2(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    let route = groups::finish_route_of(before);
    if route == 9 {
        handle_after_bundle(before);
    } else if route == 10 {
        handle_after_if(before);
    } else if route == 11 {
        handle_after_send(before);
    } else if route == 12 {
        handle_after_match_component(before);
    } else if route == 13 {
        handle_after_absent_specialized(before);
    } else if route == 14 {
        handle_after_absent_connective(before);
    } else if route == 15 {
        handle_after_receive_bind(before);
    } else if route == 16 {
        handle_after_match_case(before);
    }
}

pub proof fn handle_routed_group_3(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_routed_group_3(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    let route = groups::finish_route_of(before);
    if route == 17 {
        handle_after_bool_oneof(before);
    } else if route == 18 {
        handle_after_expr_atom(before);
    } else if route == 19 {
        handle_after_expr_numeric(before);
    } else if route == 20 {
        handle_after_expr_big_rational(before);
    } else if route == 21 {
        handle_after_expr_fixed_point(before);
    } else if route == 22 {
        handle_after_sys_auth_token(before);
    } else if route == 23 {
        handle_after_new_empty_collections(before);
    } else if route == 24 {
        handle_after_new_one_injection(before);
    }
}

pub proof fn handle_routed_group_4(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_routed_group_4(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    let route = groups::finish_route_of(before);
    if route == 25 {
        handle_after_new_one_absent_injection(before);
    } else if route == 26 {
        handle_after_new_one_uri(before);
    } else if route == 27 {
        handle_after_new_collections(before);
    } else if route == 28 {
        handle_after_expr_payload_forward(before);
    } else if route == 29 {
        handle_after_expr_structural(before);
    } else if route == 30 {
        handle_after_evar(before);
    } else if route == 31 {
        handle_after_ordered_sequence(before);
    } else if route == 32 {
        handle_after_emethod(before);
    } else if route == 33 {
        handle_after_emap_finish(before);
    }
}

pub proof fn handle_routed_group_5(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_routed_group_5(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    let route = groups::finish_route_of(before);
    if route == 34 {
        handle_after_connective_not(before);
    } else if route == 35 {
        handle_after_connective_varref(before);
    } else if route == 36 {
        handle_after_connective_collection(before);
    } else if route == 37 {
        handle_after_unforgeable_bytes(before);
    } else if route == 38 {
        handle_after_receive_present_condition(before);
    } else if route == 39 {
        handle_after_receive_absent_condition(before);
    } else if route == 40 {
        handle_after_nonsemantic(before);
    }
}

pub proof fn handle_routed_group_6(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match groups::finish_routed_group_6(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    let route = groups::finish_route_of(before);
    if route == 41 {
        handle_after_var(before);
    } else if route == 42 {
        handle_after_epathmap_owner(before);
    } else if route == 43 {
        handle_after_ezipper(before);
    }
}

/// Every selected generic finish has the top-handle facts: the selector returns the routed
/// candidate (`bridge_score_finish_never_ambiguous`), the route's class names its group, and the
/// group lemma covers the routed kind.
pub proof fn handle_after_selected_finish(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match bridge::bridge_score_finish_selected(before) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    reveal(groups::finish_top_wf);
    assert(admission::admission_frame_wf(
        before.admission.frames[before.admission.frames.len() - 1],
    ));
    assert(groups::finish_top_wf(before));
    unique::bridge_score_finish_never_ambiguous(before);
    if groups::finish_class_1(before) {
        handle_routed_group_1(before);
    } else if groups::finish_class_2(before) {
        handle_routed_group_2(before);
    } else if groups::finish_class_3(before) {
        handle_routed_group_3(before);
    } else if groups::finish_class_4(before) {
        handle_routed_group_4(before);
    } else if groups::finish_class_5(before) {
        handle_routed_group_5(before);
    } else if groups::finish_class_6(before) {
        handle_routed_group_6(before);
    }
}

/// The EPathMap source finish of an `EofEPathMap` event ends in constructor 13, so its top handle
/// has the facts too.
pub proof fn handle_after_epm_source_finish(
    before: runtime_source::RuntimeFrameImage, snapshot_bytes: Seq<u8>,
)
    ensures match runtime_source::runtime_epm_source_score_finish_selected(
        before, snapshot_bytes,
    ) {
        None => true,
        Some(after) => finish_top_handle(after),
    },
{
    runtime_source::runtime_epm_source_score_finish_is_checked(before, snapshot_bytes);
    attach_agreement_everywhere();
    if let Some(after) = runtime_source::runtime_epm_source_score_finish_selected(
        before, snapshot_bytes,
    ) {
        top_handle_from_agreement(after);
    }
}

/// The root module's product-level form of that finish
/// (`product_epm_source_bridge_finish_selected`) also ends in constructor 13, so its top handle has
/// the facts.
pub proof fn handle_after_product_epm_finish(before: crate::ProductScoreState)
    ensures match crate::product_epm_source_bridge_finish_selected(before) {
        None => true,
        Some((after, _, _, _, _)) => finish_top_handle(after),
    },
{
    crate::product_epm_source_bridge_finish_selected_exact(before);
    attach_agreement_everywhere();
    if let Some((after, _, _, _, _)) = crate::product_epm_source_bridge_finish_selected(before) {
        top_handle_from_agreement(after);
    }
}

/// A child pop accepts a summary only if it agrees with the child
/// (`admission_child_summary_agrees`): the parent's new return carries the child's handle, body id
/// and bytes, with no origin when unscored and with the arena's body id and the top-handle facts
/// when scored. The arena is unchanged.
pub proof fn child_return_carries_handle(
    pre_pop: bridge::ScoreAdmissionBridge, returned_id: u64, returned_kind: u8,
    summary: admission::AdmissionReturnedKind,
)
    requires bridge::bridge_well_formed(pre_pop),
    ensures match admission::admission_complete_child_step(
        pre_pop.admission, returned_id, returned_kind, summary,
    ) {
        None => true,
        Some(next) => {
            &&& next.arena == pre_pop.admission.arena
            &&& next.frames.len() > 0
            &&& next.frames.last().returns.len() > 0
            &&& next.frames.last().returns.last().kind == summary
            &&& admission::admission_summary_has_score(summary)
                == pre_pop.admission.frames.last().score_handle.is_some()
            &&& finish_child_return_facts(
                pre_pop.admission.frames.last(),
                bridge::ScoreAdmissionBridge { admission: next, transcript: pre_pop.transcript },
            )
        },
    },
{
    if let Some(next) = admission::admission_complete_child_step(
        pre_pop.admission, returned_id, returned_kind, summary,
    ) {
        let state = pre_pop.admission;
        let child = state.frames[state.frames.len() - 1];
        let parent_index = state.frames.len() - 2;
        let popped = bridge::ScoreAdmissionBridge {
            admission: next,
            transcript: pre_pop.transcript,
        };
        assert(child == state.frames.last());
        assert(next.frames.len() == state.frames.len() - 1);
        assert(next.frames.last() == next.frames[parent_index]);
        assert(next.frames[parent_index].returns.len()
            == state.frames[parent_index].returns.len() + 1);
        let returned = next.frames[parent_index].returns.last();
        assert(returned.score_handle == child.score_handle);
        assert(returned.body_id == child.body_id);
        assert(returned.body_bytes == child.source_bytes);
        assert(returned.kind == summary);
        assert(admission::admission_child_summary_agrees(child, summary, state.arena));
        if admission::admission_summary_has_score(summary) {
            handle_facts_from_agreement(pre_pop, child);
            assert(finish_handle_facts(popped, child, child.score_handle.unwrap()));
        }
    }
}

/// Every runtime child pop is a `Complete` of the cursor model, whose projection is
/// `admission_complete_child_step` on the derived summary: the parent's new return carries the
/// child's facts, and the arena and transcript are unchanged.
pub proof fn pre_pop_return_carries_handle(pre_pop: runtime_source::RuntimeFrameImage)
    ensures match runtime_source::runtime_pre_pop_child_accept(pre_pop) {
        None => true,
        Some(image) => {
            &&& image.score.transcript == pre_pop.score.transcript
            &&& image.score.admission.arena == pre_pop.score.admission.arena
            &&& finish_child_return_facts(pre_pop.score.admission.frames.last(), image.score)
        },
    },
{
    reveal(runtime_source::runtime_pre_pop_child_accept);
    if let Some(image) = runtime_source::runtime_pre_pop_child_accept(pre_pop) {
        let state = pre_pop.score.admission;
        admission::concrete_message_complete_child_simulates(state);
        let child = state.frames[state.frames.len() - 1];
        let parent = state.frames[state.frames.len() - 2];
        let summary = admission::concrete_derived_summary_with_arena(
            parent, child, state.arena,
        ).unwrap();
        child_return_carries_handle(
            pre_pop.score, child.return_id.unwrap(), child.frame_kind, summary,
        );
        assert(image.score.admission == admission::admission_complete_child_step(
            state, child.return_id.unwrap(), child.frame_kind, summary,
        ).unwrap());
    }
}

/// The scored child pop after a selected finish (`runtime_message_eof_child_accept`) is the pop of
/// the finished state, so the parent's new return carries the finished top's facts.
pub proof fn eof_score_child_carries_handle(
    before: runtime_source::RuntimeFrameImage, after_finish: bridge::ScoreAdmissionBridge,
)
    ensures match runtime_source::runtime_message_eof_child_accept(before, after_finish) {
        None => true,
        Some(image) => {
            &&& image.score.transcript == after_finish.transcript
            &&& image.score.admission.arena == after_finish.admission.arena
            &&& finish_child_return_facts(after_finish.admission.frames.last(), image.score)
        },
    },
{
    reveal(runtime_source::runtime_message_eof_child_accept);
    pre_pop_return_carries_handle(
        runtime_source::RuntimeFrameImage { score: after_finish, ..before },
    );
}

/// A root return whose one pre-pop frame finished with the top-handle facts, whose summary handle
/// is that frame's and whose bytes are the arena binding's, has the root return facts.
pub proof fn root_return_from_top_handle(returned: runtime_source::RuntimeRootReturnImage)
    requires
        finish_top_handle(returned.pre_pop.score),
        returned.remaining_frames == 0,
        returned.pre_pop.score.admission.frames.len() == 1,
        returned.summary.score_handle
            == returned.pre_pop.score.admission.frames[0].score_handle.unwrap(),
        returned.score_bytes == returned.pre_pop.score.admission.arena[
            returned.summary.score_handle as int
        ].transcript_bytes,
    ensures finish_root_return_facts(returned),
{
    reveal(finish_top_handle);
    let scored = returned.pre_pop.score;
    assert(scored.admission.frames.last() == scored.admission.frames[0]);
}

/// Every `Eof` outcome of the runtime driver states the facts of its finish: a scored child return
/// (the selected finish has the top-handle facts and the parent's new return carries them), an
/// unscored child return (no handle and no origin), a scored root return (the root return facts),
/// and an unscored root return (no selected finish and no handle).
pub proof fn runtime_eof_finish_facts(before: runtime_source::RuntimeFrameImage)
    requires runtime_source::runtime_frame_image_wf(before),
    ensures match runtime_source::runtime_message_driver_step(
        before, runtime_source::RuntimeMessageEvent::Eof,
    ) {
        None => true,
        Some(runtime_source::RuntimeMessageOutcome::ParentAccepted { image }) =>
            match bridge::bridge_score_finish_selected(before.score) {
                None => {
                    &&& before.score.admission.frames.last().score_handle.is_none()
                    &&& image.score.transcript == before.score.transcript
                    &&& image.score.admission.arena == before.score.admission.arena
                    &&& finish_child_return_facts(
                        before.score.admission.frames.last(), image.score,
                    )
                },
                Some(after_finish) => {
                    &&& finish_top_handle(after_finish)
                    &&& image.score.transcript == after_finish.transcript
                    &&& image.score.admission.arena == after_finish.admission.arena
                    &&& finish_child_return_facts(
                        after_finish.admission.frames.last(), image.score,
                    )
                },
            },
        Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }) =>
            match bridge::bridge_score_finish_selected(before.score) {
                None => false,
                Some(after_finish) => {
                    &&& finish_top_handle(after_finish)
                    &&& returned.pre_pop.score == after_finish
                    &&& finish_root_return_facts(returned)
                },
            },
        Some(runtime_source::RuntimeMessageOutcome::RootReturnedNoScore { returned }) => {
            &&& bridge::bridge_score_finish_selected(before.score).is_none()
            &&& returned.pre_pop == before
            &&& before.score.admission.frames[0].score_handle.is_none()
            &&& !admission::admission_summary_has_score(returned.summary.kind)
        },
        _ => false,
    },
{
    runtime_source::runtime_selected_eof_outcome_is_checked(before);
    if runtime_source::runtime_noscore_child_kind(before).is_some() {
        pre_pop_return_carries_handle(before);
    } else if runtime_source::runtime_noscore_root_return(before).is_none() {
        handle_after_selected_finish(before.score);
        if let Some(after_finish) = bridge::bridge_score_finish_selected(before.score) {
            if before.score.admission.frames.len() == 1 {
                if let Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }) =
                    runtime_source::runtime_message_driver_step(
                        before, runtime_source::RuntimeMessageEvent::Eof,
                    ) {
                    root_return_from_top_handle(returned);
                }
            } else {
                eof_score_child_carries_handle(before, after_finish);
            }
        }
    }
}

/// Every `EofEPathMap` outcome states the facts of the EPathMap source finish: a child return
/// carries them into the parent, and a root return has the root return facts.
pub proof fn runtime_epathmap_eof_finish_facts(
    before: runtime_source::RuntimeFrameImage, snapshot_bytes: Seq<u8>,
)
    ensures match runtime_source::runtime_message_driver_step(
        before, runtime_source::RuntimeMessageEvent::EofEPathMap { snapshot_bytes },
    ) {
        None => true,
        Some(runtime_source::RuntimeMessageOutcome::ParentAccepted { image }) =>
            match runtime_source::runtime_epm_source_score_finish_selected(
                before, snapshot_bytes,
            ) {
                None => false,
                Some(after_finish) => {
                    &&& finish_top_handle(after_finish)
                    &&& image.score.transcript == after_finish.transcript
                    &&& image.score.admission.arena == after_finish.admission.arena
                    &&& finish_child_return_facts(
                        after_finish.admission.frames.last(), image.score,
                    )
                },
            },
        Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }) =>
            match runtime_source::runtime_epm_source_score_finish_selected(
                before, snapshot_bytes,
            ) {
                None => false,
                Some(after_finish) => {
                    &&& finish_top_handle(after_finish)
                    &&& returned.pre_pop.score == after_finish
                    &&& finish_root_return_facts(returned)
                },
            },
        _ => false,
    },
{
    handle_after_epm_source_finish(before, snapshot_bytes);
    if let Some(after_finish) = runtime_source::runtime_epm_source_score_finish_selected(
        before, snapshot_bytes,
    ) {
        if before.score.admission.frames.len() == 1 {
            admission::concrete_message_driver_step_simulates(
                after_finish.admission, admission::ConcreteFrameActionModel::Complete,
            );
            if let Some(runtime_source::RuntimeMessageOutcome::RootReturned { returned }) =
                runtime_source::runtime_message_driver_step(
                    before, runtime_source::RuntimeMessageEvent::EofEPathMap { snapshot_bytes },
                ) {
                root_return_from_top_handle(returned);
            }
        } else {
            pre_pop_return_carries_handle(
                runtime_source::RuntimeFrameImage { score: after_finish, ..before },
            );
        }
    }
}

/// The live root score step installs the selected finish of the old bridge, so the new bridge's
/// top handle has the facts.
pub proof fn live_root_score_has_top_handle(
    before: crate::LiveRuntimeProductState, after: crate::LiveRuntimeProductState,
)
    requires crate::live_runtime_product_root_score_step(before, after),
    ensures finish_top_handle(after.source.product.bridge),
{
    assert(runtime_source::runtime_frame_image_wf(before.source.runtime));
    assert(before.source.runtime.score == before.source.product.bridge);
    handle_after_selected_finish(before.source.product.bridge);
}

/// The live child score step (production message EOF before the pop) installs the selected finish
/// of the old bridge, so the new bridge's top handle has the facts.
pub proof fn live_child_score_has_top_handle(
    before: crate::LiveRuntimeProductState, after: crate::LiveRuntimeProductState,
)
    requires crate::live_runtime_product_message_child_score_step(before, after),
    ensures finish_top_handle(after.source.product.bridge),
{
    assert(runtime_source::runtime_frame_image_wf(before.source.runtime));
    assert(before.source.runtime.score == before.source.product.bridge);
    handle_after_selected_finish(before.source.product.bridge);
}

/// The live root completion (score and return in one production operation) returns a root with
/// the root return facts, over the scored pre-pop bridge.
pub proof fn live_root_complete_carries_handle(
    before: crate::LiveRuntimeProductState, after: crate::LiveRuntimeProductState,
    returned: runtime_source::RuntimeRootReturnImage,
)
    requires crate::live_runtime_product_root_complete_step(before, after, returned),
    ensures
        finish_top_handle(after.source.product.bridge),
        returned.pre_pop.score == after.source.product.bridge,
        finish_root_return_facts(returned),
{
    assert(runtime_source::runtime_frame_image_wf(before.source.runtime));
    assert(before.source.runtime.score == before.source.product.bridge);
    runtime_eof_finish_facts(before.source.runtime);
}

/// Non-vacuity: on the modeled EPathMap owner at its EOF (two frames, so a child finish), the
/// selected finish exists and has the top-handle facts.
pub proof fn handle_owner_witness()
    ensures
        unique::finish_owner_top_state().admission.frames.len() == 2,
        bridge::bridge_score_finish_selected(unique::finish_owner_top_state())
            == Some(unique::finish_owner_top_scored()),
        finish_top_handle(unique::finish_owner_top_scored()),
{
    unique::finish_owner_top_instance();
    unique::finish_owner_top_shape();
    let before = unique::finish_owner_top_state();
    groups::finish_eof_prestate_top_wf(before);
    handle_after_selected_finish(before);
}

}
