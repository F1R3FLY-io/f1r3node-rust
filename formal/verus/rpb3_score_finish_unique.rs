//! Open obligation 2 of `docs/design/pathmap/rpb3-admission-source-traceability.md`, part
//! 2b: composed from the six score-finish groups of `rpb3_score_finish_groups` (part 2a), the
//! fold inside `bridge_score_finish_selected` ends in `Unique(after)` on every well-formed EOF
//! prestate with a successful generic score finish `after`, so it is never `NoCandidate` and
//! never `Ambiguous` there (`bridge_score_finish_unique`). On every state whose top frame is
//! well formed the fold is never `Ambiguous` and selects the routed candidate
//! (`bridge_score_finish_never_ambiguous`).
//!
//! `bridge_score_finish_epathmap_returns` covers the nested EPathMap return path frame by frame:
//! the EPM1 snapshot child has no generic finish (`EpmFinishAccept` completes it); the program-31
//! owner (production's `reduce_epathmap`) and the program-32 EZipper (`reduce_ezipper`) take
//! their group-6 finishes, both scored like production's `rpb3_pathmap_score`; the Expr payload
//! parent takes the payload forward finish, or the nonsemantic finish while incomplete; a
//! program-1 value child takes the raw Par finish. Instances on concrete states:
//! - the root module's modeled nested EPathMap source with the snapshot child on top
//!   (`finish_epathmap_nested_witness`: no generic finish);
//! - the same source after its EPathMap finish accept, owner on top
//!   (`finish_owner_top_instance`: `Unique` with exactly the score of the root module's
//!   EPathMap-specific `product_epm_source_bridge_finish_selected`);
//! - that owner completed into its Expr parent (`finish_payload_parent_complete_instance`);
//! - an EZipper that read no field (`finish_ezipper_top_instance`: no generic finish) and the
//!   bridge's scored EZipper (`finish_ezipper_scored_instance`: `Unique`);
//! - an unscored payload parent and a value child.
//!
//! Group 6 makes three earlier statements false: the owner and EZipper clauses of
//! `bridge_score_finish_epathmap_returns` (`NoCandidate`), `finish_epathmap_owner_product` and
//! `finish_owner_top_instance`. Each is commented out with its reason and followed by the
//! corrected lemma of the same name.
//!
//! The concrete states are opaque; only their shape lemmas reveal them.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_epm_source_accept_refinement as epm_source_accept,
    rpb3_map_frame_refinement as map, rpb3_score_admission_bridge as bridge,
    rpb3_score_finish_groups as groups, rpb3_score_transcript as score,
};

verus! {

/// The fold of `bridge::bridge_score_finish_selected` over its six groups, restated so that the
/// final selection can be named.
pub open spec fn finish_selection(before: bridge::ScoreAdmissionBridge)
    -> bridge::BridgeScoreFinishSelection {
    let selection_0 = bridge::BridgeScoreFinishSelection::NoCandidate;
    let selection_1 = bridge::bridge_score_finish_selection_group_1(before, selection_0);
    let selection_2 = bridge::bridge_score_finish_selection_group_2(before, selection_1);
    let selection_3 = bridge::bridge_score_finish_selection_group_3(before, selection_2);
    let selection_4 = bridge::bridge_score_finish_selection_group_4(before, selection_3);
    let selection_5 = bridge::bridge_score_finish_selection_group_5(before, selection_4);
    bridge::bridge_score_finish_selection_group_6(before, selection_5)
}

/// The selector returns the fold's candidate when the fold ends in `Unique`, and `None` otherwise.
pub proof fn finish_selected_unfolds(before: bridge::ScoreAdmissionBridge)
    ensures
        bridge::bridge_score_finish_selected(before) == match finish_selection(before) {
            bridge::BridgeScoreFinishSelection::Unique(next) => Some(next),
            _ => None,
        },
{
    reveal(bridge::bridge_score_finish_selected);
}

/// The whole fold adds exactly one optional candidate, the routed one: each group adds its routed
/// candidate (`finish_group_K_folds`), and the route lies in at most one group's range.
pub proof fn finish_selection_folds(before: bridge::ScoreAdmissionBridge)
    requires groups::finish_top_wf(before),
    ensures
        finish_selection(before) == bridge::bridge_add_score_finish_candidate(
            bridge::BridgeScoreFinishSelection::NoCandidate, groups::finish_routed(before),
        ),
{
    reveal(groups::finish_top_wf);
    let selection_0 = bridge::BridgeScoreFinishSelection::NoCandidate;
    groups::finish_group_1_folds(before, selection_0);
    let selection_1 = bridge::bridge_score_finish_selection_group_1(before, selection_0);
    groups::finish_group_2_folds(before, selection_1);
    let selection_2 = bridge::bridge_score_finish_selection_group_2(before, selection_1);
    groups::finish_group_3_folds(before, selection_2);
    let selection_3 = bridge::bridge_score_finish_selection_group_3(before, selection_2);
    groups::finish_group_4_folds(before, selection_3);
    let selection_4 = bridge::bridge_score_finish_selection_group_4(before, selection_3);
    groups::finish_group_5_folds(before, selection_4);
    let selection_5 = bridge::bridge_score_finish_selection_group_5(before, selection_4);
    groups::finish_group_6_folds(before, selection_5);
}

/// Never `Ambiguous` on a state whose top frame is well formed: the fold is `NoCandidate` exactly
/// when the routed candidate is absent, and the selector returns the routed candidate.
pub proof fn bridge_score_finish_never_ambiguous(before: bridge::ScoreAdmissionBridge)
    requires groups::finish_top_wf(before),
    ensures
        finish_selection(before) != bridge::BridgeScoreFinishSelection::Ambiguous,
        (finish_selection(before) == bridge::BridgeScoreFinishSelection::NoCandidate)
            == groups::finish_routed(before).is_none(),
        bridge::bridge_score_finish_selected(before) == groups::finish_routed(before),
{
    finish_selection_folds(before);
    finish_selected_unfolds(before);
}

/// Part 2b. On every well-formed EOF prestate with a successful generic score finish `after`, the
/// fold ends in `Unique(after)`. Composition of part 2a: the six classes partition such prestates
/// (`finish_classes_partition`), the group of the prestate's class adds `after`
/// (`finish_group_K_covers`), and every other group leaves the selection unchanged
/// (`finish_group_K_disjoint`). So the fold is neither `NoCandidate` nor `Ambiguous`, the selector
/// returns `after`, and `after` is the only successful generic finish.
pub proof fn bridge_score_finish_unique(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires
        groups::finish_eof_prestate(before),
        bridge::bridge_score_source_step(before, after),
    ensures
        finish_selection(before) == bridge::BridgeScoreFinishSelection::Unique(after),
        finish_selection(before) != bridge::BridgeScoreFinishSelection::NoCandidate,
        finish_selection(before) != bridge::BridgeScoreFinishSelection::Ambiguous,
        bridge::bridge_score_finish_selected(before) == Some(after),
        forall|other: bridge::ScoreAdmissionBridge|
            #[trigger] bridge::bridge_score_source_step(before, other) ==> other == after,
{
    groups::finish_classes_partition(before, after);
    groups::finish_group_1_disjoint(before);
    groups::finish_group_2_disjoint(before);
    groups::finish_group_3_disjoint(before);
    groups::finish_group_4_disjoint(before);
    groups::finish_group_5_disjoint(before);
    groups::finish_group_6_disjoint(before);
    if groups::finish_class_1(before) {
        groups::finish_group_1_covers(before, after);
    } else if groups::finish_class_2(before) {
        groups::finish_group_2_covers(before, after);
    } else if groups::finish_class_3(before) {
        groups::finish_group_3_covers(before, after);
    } else if groups::finish_class_4(before) {
        groups::finish_group_4_covers(before, after);
    } else if groups::finish_class_5(before) {
        groups::finish_group_5_covers(before, after);
    } else {
        groups::finish_group_6_covers(before, after);
    }
    finish_selected_unfolds(before);
    groups::finish_eof_prestate_top_wf(before);
    assert forall|other: bridge::ScoreAdmissionBridge|
        #[trigger] bridge::bridge_score_source_step(before, other) implies other == after by {
        groups::finish_source_step_routes(before, other);
        groups::finish_source_step_routes(before, after);
    }
}

/// The EPM1 snapshot child pushed for descriptor (31, 9): frame kind 3 and program 0, because
/// that descriptor has no child type.
pub open spec fn finish_epathmap_snapshot_child(top: admission::AdmissionFrameState) -> bool {
    &&& top.family == admission::AdmissionFamily::Other
    &&& top.frame_kind == 3
    &&& top.program_type_id == 0
}

/// The program-31 EPathMap message that owns the snapshot child.
pub open spec fn finish_epathmap_owner(top: admission::AdmissionFrameState) -> bool {
    &&& top.family == admission::AdmissionFamily::Other
    &&& top.frame_kind == 1
    &&& top.program_type_id == 31
}

/// The program-32 EZipper message, the other parent of a program-31 return.
pub open spec fn finish_ezipper_owner(top: admission::AdmissionFrameState) -> bool {
    &&& top.family == admission::AdmissionFamily::Other
    &&& top.frame_kind == 1
    &&& top.program_type_id == 32
}

/// An Expr frame whose only return is the program-31 payload at tag 32.
pub open spec fn finish_epathmap_payload_parent(top: admission::AdmissionFrameState) -> bool {
    &&& top.family == admission::AdmissionFamily::Expr
    &&& top.scalar_occurrences.len() == 0
    &&& top.returns.len() == 1
    &&& top.returns[0].tag == 32
    &&& match top.returns[0].kind {
        admission::AdmissionReturnedKind::ExprPayload { .. } => true,
        _ => false,
    }
}

/// A ParValue or RawParEscape child of an EPathMap entry: a program-1 Par frame.
pub open spec fn finish_epathmap_value_child(top: admission::AdmissionFrameState) -> bool {
    &&& top.family == admission::AdmissionFamily::Other
    &&& top.frame_kind == 1
    &&& top.program_type_id == 1
}

// Disabled: false with group 6, owner and EZipper finishes fire; see the same-name lemma below.
// pub proof fn bridge_score_finish_epathmap_returns(before: bridge::ScoreAdmissionBridge)
//     requires groups::finish_top_wf(before),
//     ensures
//         finish_selection(before) != bridge::BridgeScoreFinishSelection::Ambiguous,
//         finish_epathmap_snapshot_child(before.admission.frames.last())
//             || finish_epathmap_owner(before.admission.frames.last())
//             || finish_ezipper_owner(before.admission.frames.last()) ==> {
//             &&& finish_selection(before) == bridge::BridgeScoreFinishSelection::NoCandidate
//             &&& bridge::bridge_score_finish_selected(before).is_none()
//             &&& forall|after: bridge::ScoreAdmissionBridge|
//                 !#[trigger] bridge::bridge_score_source_step(before, after)
//         },
//         finish_epathmap_payload_parent(before.admission.frames.last()) ==> finish_selection(before)
//             == bridge::bridge_add_score_finish_candidate(
//                 bridge::BridgeScoreFinishSelection::NoCandidate,
//                 if admission::admission_family_complete(before.admission.frames.last()) {
//                     groups::finish_candidate_expr_payload_forward(before)
//                 } else {
//                     groups::finish_candidate_nonsemantic(before)
//                 },
//             ),
//         finish_epathmap_value_child(before.admission.frames.last()) ==> finish_selection(before)
//             == bridge::bridge_add_score_finish_candidate(
//                 bridge::BridgeScoreFinishSelection::NoCandidate,
//                 groups::finish_candidate_raw_par(before),
//             ),
// {
//     hide(bridge::bridge_score_source_step);
//     hide(bridge::bridge_raw_par_step);
//     hide(bridge::bridge_cost_atom_absent_step);
//     hide(bridge::bridge_cost_atom_bytes_step);
//     hide(bridge::bridge_cost_bound_step);
//     hide(bridge::bridge_cost_atom_bound_forward_step);
//     hide(bridge::bridge_cost_signature_step);
//     hide(bridge::bridge_cost_region_step);
//     hide(bridge::bridge_cost_token_stack_step);
//     hide(bridge::bridge_bundle_step);
//     hide(bridge::bridge_if_step);
//     hide(bridge::bridge_send_step);
//     hide(bridge::bridge_match_step);
//     hide(bridge::bridge_absent_specialized_constructor_step);
//     hide(bridge::bridge_absent_connective_step);
//     hide(bridge::bridge_receive_bind_step);
//     hide(bridge::bridge_match_case_step);
//     hide(bridge::bridge_bool_oneof_step);
//     hide(bridge::bridge_expr_atom_step);
//     hide(bridge::bridge_expr_numeric_step);
//     hide(bridge::bridge_expr_big_rational_step);
//     hide(bridge::bridge_expr_fixed_point_step);
//     hide(bridge::bridge_sys_auth_token_step);
//     hide(bridge::bridge_new_empty_collections_step);
//     hide(bridge::bridge_new_one_injection_step);
//     hide(bridge::bridge_new_one_absent_injection_step);
//     hide(bridge::bridge_new_one_uri_step);
//     hide(bridge::bridge_new_collections_step);
//     hide(bridge::bridge_expr_payload_forward_step);
//     hide(bridge::bridge_expr_structural_step);
//     hide(bridge::bridge_evar_step);
//     hide(bridge::bridge_ordered_sequence_step);
//     hide(bridge::bridge_emethod_step);
//     hide(bridge::bridge_emap_finish_step);
//     hide(bridge::bridge_connective_not_step);
//     hide(bridge::bridge_connective_varref_step);
//     hide(bridge::bridge_connective_collection_step);
//     hide(bridge::bridge_unforgeable_bytes_step);
//     hide(bridge::bridge_receive_present_condition_step);
//     hide(bridge::bridge_receive_absent_condition_step);
//     hide(bridge::bridge_nonsemantic_step);
//     reveal(groups::finish_route);
//     bridge_score_finish_never_ambiguous(before);
//     finish_selection_folds(before);
//     let top = before.admission.frames.last();
//     if finish_epathmap_payload_parent(top) {
//         assert(top.seen.contains(top.returns[0].slot as int));
//     }
//     if finish_epathmap_snapshot_child(top) || finish_epathmap_owner(top)
//         || finish_ezipper_owner(top)
//     {
//         assert(groups::finish_route_of(before) == 0);
//         assert forall|after: bridge::ScoreAdmissionBridge|
//             !#[trigger] bridge::bridge_score_source_step(before, after) by {
//             if bridge::bridge_score_source_step(before, after) {
//                 groups::finish_source_step_routes(before, after);
//             }
//         }
//     }
// }

/// Every frame on the nested EPathMap return path has at most one generic score finish, and the
/// fold adds exactly that one: none for the snapshot child (the EPathMap finish accept completes
/// it), the group-6 owner finish for the program-31 owner, the group-6 EZipper finish for the
/// program-32 EZipper, the payload forward finish for a complete Expr payload parent (the
/// nonsemantic finish while it is incomplete), and the raw Par finish for a value child.
pub proof fn bridge_score_finish_epathmap_returns(before: bridge::ScoreAdmissionBridge)
    requires groups::finish_top_wf(before),
    ensures
        finish_selection(before) != bridge::BridgeScoreFinishSelection::Ambiguous,
        finish_epathmap_snapshot_child(before.admission.frames.last()) ==> {
            &&& finish_selection(before) == bridge::BridgeScoreFinishSelection::NoCandidate
            &&& bridge::bridge_score_finish_selected(before).is_none()
            &&& forall|after: bridge::ScoreAdmissionBridge|
                !#[trigger] bridge::bridge_score_source_step(before, after)
        },
        finish_epathmap_owner(before.admission.frames.last()) ==> finish_selection(before)
            == bridge::bridge_add_score_finish_candidate(
                bridge::BridgeScoreFinishSelection::NoCandidate,
                groups::finish_candidate_epathmap_owner(before),
            ),
        finish_ezipper_owner(before.admission.frames.last()) ==> finish_selection(before)
            == bridge::bridge_add_score_finish_candidate(
                bridge::BridgeScoreFinishSelection::NoCandidate,
                groups::finish_candidate_ezipper(before),
            ),
        finish_epathmap_payload_parent(before.admission.frames.last()) ==> finish_selection(before)
            == bridge::bridge_add_score_finish_candidate(
                bridge::BridgeScoreFinishSelection::NoCandidate,
                if admission::admission_family_complete(before.admission.frames.last()) {
                    groups::finish_candidate_expr_payload_forward(before)
                } else {
                    groups::finish_candidate_nonsemantic(before)
                },
            ),
        finish_epathmap_value_child(before.admission.frames.last()) ==> finish_selection(before)
            == bridge::bridge_add_score_finish_candidate(
                bridge::BridgeScoreFinishSelection::NoCandidate,
                groups::finish_candidate_raw_par(before),
            ),
{
    reveal(groups::finish_top_wf);
    reveal(groups::finish_route);
    bridge_score_finish_never_ambiguous(before);
    finish_selection_folds(before);
    let top = before.admission.frames.last();
    if finish_epathmap_payload_parent(top) {
        assert(top.seen.contains(top.returns[0].slot as int));
    }
    if finish_epathmap_snapshot_child(top) {
        assert(groups::finish_route_of(before) == 0);
        assert forall|after: bridge::ScoreAdmissionBridge|
            !#[trigger] bridge::bridge_score_source_step(before, after) by {
            if bridge::bridge_score_source_step(before, after) {
                groups::finish_source_step_routes(before, after);
            }
        }
    }
}

/// The root module's modeled nested EPathMap source as a score bridge with an empty transcript:
/// an Expr waiting on tag 32, the program-31 owner waiting on slot 3 / tag 9, and the EPM1
/// snapshot child at its EOF on top.
#[verifier::opaque]
pub open spec fn finish_epathmap_nested_prestate() -> bridge::ScoreAdmissionBridge {
    bridge::ScoreAdmissionBridge {
        admission: crate::product_epm_modeled_empty_nested_source().admission,
        transcript: score::empty_arena(),
    }
}

/// The nested prestate is a well-formed EOF prestate of three frames: the snapshot child on top,
/// the program-31 owner below it.
pub proof fn finish_epathmap_nested_shape()
    ensures
        groups::finish_eof_prestate(finish_epathmap_nested_prestate()),
        finish_epathmap_nested_prestate().admission.frames.len() == 3,
        finish_epathmap_snapshot_child(finish_epathmap_nested_prestate().admission.frames.last()),
        finish_epathmap_owner(finish_epathmap_nested_prestate().admission.frames[1]),
{
    reveal(finish_epathmap_nested_prestate);
    reveal(groups::finish_eof_prestate);
    crate::product_epm_modeled_empty_nested_source_accepts();
    score::empty_arena_well_formed();
    let before = finish_epathmap_nested_prestate();
    assert(before.admission.frames.len() == 3);
    assert(bridge::bridge_well_formed(before));
    assert(before.admission.frames.last() == before.admission.frames[2]);
}

/// Non-vacuity of the snapshot-child clause: on the modeled nested prestate the generic fold ends
/// in `NoCandidate` and no generic finish exists, so only the EPathMap finish accept
/// (`EpmFinishAccept`) completes the snapshot child.
pub proof fn finish_epathmap_nested_witness()
    ensures
        groups::finish_eof_prestate(finish_epathmap_nested_prestate()),
        finish_epathmap_snapshot_child(finish_epathmap_nested_prestate().admission.frames.last()),
        finish_epathmap_owner(finish_epathmap_nested_prestate().admission.frames[1]),
        finish_selection(finish_epathmap_nested_prestate())
            == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selected(finish_epathmap_nested_prestate()).is_none(),
        forall|after: bridge::ScoreAdmissionBridge|
            !#[trigger] bridge::bridge_score_source_step(finish_epathmap_nested_prestate(), after),
{
    finish_epathmap_nested_shape();
    let before = finish_epathmap_nested_prestate();
    groups::finish_eof_prestate_top_wf(before);
    bridge_score_finish_epathmap_returns(before);
}

// Disabled: false with group 6, the owner finish can fire there; see the same-name lemma below.
// pub proof fn finish_epathmap_owner_product(before: crate::ProductScoreState)
//     requires
//         crate::product_score_wf(before),
//         before.bridge.admission.frames.len() > 0,
//         finish_epathmap_owner(before.bridge.admission.frames.last()),
//     ensures
//         finish_selection(before.bridge) == bridge::BridgeScoreFinishSelection::NoCandidate,
//         bridge::bridge_score_finish_selected(before.bridge).is_none(),
//         forall|after: bridge::ScoreAdmissionBridge|
//             !#[trigger] bridge::bridge_score_source_step(before.bridge, after),
// {
//     hide(bridge::bridge_score_source_step);
//     hide(crate::product_epm_source_receipts_wf);
//     hide(crate::product_frame_source_anchors_match);
//     hide(epm::indexed_image_wf);
//     hide(epm::frame_layout_wf);
//     assert(map::integrated_admission_wf(before.map));
//     assert(admission::admission_state_wf(before.bridge.admission));
//     let last = before.bridge.admission.frames.len() - 1;
//     assert(admission::admission_frame_wf(before.bridge.admission.frames[last]));
//     assert(groups::finish_top_wf(before.bridge));
//     bridge_score_finish_epathmap_returns(before.bridge);
// }

/// Product-level reading of the owner clause: on a well-formed product state whose top frame is
/// the program-31 owner, the generic fold adds exactly the group-6 owner finish, the selector
/// returns it, and every successful generic finish is it.
pub proof fn finish_epathmap_owner_product(before: crate::ProductScoreState)
    requires
        crate::product_score_wf(before),
        before.bridge.admission.frames.len() > 0,
        finish_epathmap_owner(before.bridge.admission.frames.last()),
    ensures
        finish_selection(before.bridge) == bridge::bridge_add_score_finish_candidate(
            bridge::BridgeScoreFinishSelection::NoCandidate,
            groups::finish_candidate_epathmap_owner(before.bridge),
        ),
        bridge::bridge_score_finish_selected(before.bridge)
            == groups::finish_candidate_epathmap_owner(before.bridge),
        forall|after: bridge::ScoreAdmissionBridge|
            #[trigger] bridge::bridge_score_source_step(before.bridge, after)
                ==> groups::finish_candidate_epathmap_owner(before.bridge) == Some(after),
{
    reveal(groups::finish_top_wf);
    reveal(groups::finish_route);
    assert(map::integrated_admission_wf(before.map));
    assert(admission::admission_state_wf(before.bridge.admission));
    let last = before.bridge.admission.frames.len() - 1;
    assert(admission::admission_frame_wf(before.bridge.admission.frames[last]));
    assert(groups::finish_top_wf(before.bridge));
    bridge_score_finish_epathmap_returns(before.bridge);
    finish_selected_unfolds(before.bridge);
    assert forall|after: bridge::ScoreAdmissionBridge|
        #[trigger] bridge::bridge_score_source_step(before.bridge, after)
        implies groups::finish_candidate_epathmap_owner(before.bridge) == Some(after) by {
        groups::finish_source_step_routes(before.bridge, after);
    }
}

/// On a product state where the root module's EPathMap-specific finish
/// (`product_epm_source_bridge_finish_selected`) succeeds and the top frame consumed its region,
/// the generic group-6 owner finish fires with exactly the same bridge: both append the snapshot
/// bytes of the accepted EPathMap source (which the owner's last, `EPathMapSnapshot`, return
/// carries), the connective flag and constructor 13, as production's `rpb3_pathmap_score` does for
/// `reduce_epathmap`.
pub proof fn finish_owner_score_agrees(before: crate::ProductScoreState)
    requires
        crate::product_epm_source_bridge_finish_selected(before).is_some(),
        before.bridge.admission.frames.last().cursor
            == before.bridge.admission.frames.last().region_end,
    ensures
        groups::finish_candidate_epathmap_owner(before.bridge)
            == Some(crate::product_epm_source_bridge_finish_selected(before).unwrap().0),
{
    reveal(groups::finish_candidate_epathmap_owner);
    crate::product_epm_source_bridge_finish_selected_exact(before);
    let source = before.accepted_epm_sources.last().unwrap();
    let finished = epm_source_accept::epm_end_linked(source);
    let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
    assert(epm_source_accept::epm_source_accepts_into_program31(source, finished, accepted));
    epm_source_accept::epm_source_accept_carries_end_summary(source, finished, accepted);
    let top = before.bridge.admission.frames.last();
    assert(top.returns.len() > 0);
    assert(top.returns.last() == accepted.frames.last().returns.last());
    assert(top.returns.last().body_bytes == source.epm.snapshot.bytes);
}

/// The root module's modeled nested EPathMap source after its EPathMap finish accept popped the
/// snapshot child: the Expr waiting on tag 32 below the program-31 owner, now on top at its EOF.
#[verifier::opaque]
pub open spec fn finish_owner_top_state() -> bridge::ScoreAdmissionBridge {
    crate::product_epm_nested_source_witness_after(
        crate::product_epm_modeled_empty_nested_source(),
    ).bridge
}

/// The owner-top state scored by the root module's EPathMap-specific finish
/// (`product_epm_source_bridge_finish_selected`: constructor 13 over the snapshot bytes and the
/// connective flag).
#[verifier::opaque]
pub open spec fn finish_owner_top_scored() -> bridge::ScoreAdmissionBridge {
    crate::product_epm_source_bridge_finish_selected(
        crate::product_epm_nested_source_witness_after(
            crate::product_epm_modeled_empty_nested_source(),
        ),
    ).unwrap().0
}

/// The owner-top state is a well-formed EOF prestate of two frames: the modeled Expr parent,
/// unchanged, below the program-31 owner, which consumed its region.
pub proof fn finish_owner_top_shape()
    ensures
        groups::finish_eof_prestate(finish_owner_top_state()),
        finish_owner_top_state().admission.frames.len() == 2,
        finish_owner_top_state().admission.frames[0]
            == crate::product_epm_modeled_empty_nested_source().admission.frames[0],
        finish_epathmap_owner(finish_owner_top_state().admission.frames.last()),
        finish_owner_top_state().admission.frames.last().cursor
            == finish_owner_top_state().admission.frames.last().region_end,
{
    reveal(finish_owner_top_state);
    crate::product_epm_modeled_empty_nested_source_accepts();
    crate::product_epm_modeled_empty_nested_source_lifts_accept();
    let source = crate::product_epm_modeled_empty_nested_source();
    epm_source_accept::epm_source_finish_accept_preserves_wf(source);
    let product = crate::product_epm_nested_source_witness_after(source);
    let state = finish_owner_top_state();
    let accepted = epm_source_accept::epm_source_finish_accept(source).unwrap();
    assert(state == product.bridge);
    assert(state.admission == accepted);
    assert(crate::product_score_wf(product));
    assert(bridge::bridge_well_formed(state));
    let child = source.admission.frames.last();
    let base = admission::admission_complete_child_step(
        source.admission, child.return_id.unwrap(), 3, admission::AdmissionReturnedKind::Other,
    ).unwrap();
    assert(base.frames.len() == 2);
    assert(base.frames[0] == source.admission.frames[0]);
    assert(accepted.frames.len() == 2);
    assert(accepted.frames[0] == base.frames[0]);
    let top = accepted.frames[1];
    assert(top == accepted.frames.last());
    assert(top.family == admission::AdmissionFamily::Other);
    assert(top.frame_kind == 1);
    assert(top.program_type_id == 31);
    assert(top.cursor == top.region_end);
    assert(top.waiting_id.is_none());
    assert(top.nested_started == top.nested_resolved);
    assert(top.score_handle.is_none());
    reveal(groups::finish_eof_prestate);
}

/// On the owner-top state the generic group-6 owner finish fires with exactly the root module's
/// EPathMap-specific score.
pub proof fn finish_owner_top_scores()
    ensures
        groups::finish_candidate_epathmap_owner(finish_owner_top_state())
            == Some(finish_owner_top_scored()),
{
    finish_owner_top_shape();
    reveal(finish_owner_top_state);
    reveal(finish_owner_top_scored);
    crate::product_epm_modeled_empty_nested_scores();
    let product = crate::product_epm_nested_source_witness_after(
        crate::product_epm_modeled_empty_nested_source(),
    );
    assert(crate::product_epm_source_bridge_finish_selected(product).is_some());
    finish_owner_score_agrees(product);
}

// Disabled: false with group 6, the owner-top state selects Unique; see the same-name lemma below.
// pub proof fn finish_owner_top_instance()
//     ensures
//         groups::finish_eof_prestate(finish_owner_top_state()),
//         groups::finish_top_wf(finish_owner_top_state()),
//         finish_epathmap_owner(finish_owner_top_state().admission.frames.last()),
//         finish_selection(finish_owner_top_state())
//             == bridge::BridgeScoreFinishSelection::NoCandidate,
//         bridge::bridge_score_finish_selected(finish_owner_top_state()).is_none(),
//         forall|after: bridge::ScoreAdmissionBridge|
//             !#[trigger] bridge::bridge_score_source_step(finish_owner_top_state(), after),
// {
//     hide(bridge::bridge_score_source_step);
//     hide(bridge::bridge_raw_par_step);
//     hide(bridge::bridge_cost_atom_absent_step);
//     hide(bridge::bridge_cost_atom_bytes_step);
//     hide(bridge::bridge_cost_bound_step);
//     hide(bridge::bridge_cost_atom_bound_forward_step);
//     hide(bridge::bridge_cost_signature_step);
//     hide(bridge::bridge_cost_region_step);
//     hide(bridge::bridge_cost_token_stack_step);
//     hide(bridge::bridge_bundle_step);
//     hide(bridge::bridge_if_step);
//     hide(bridge::bridge_send_step);
//     hide(bridge::bridge_match_step);
//     hide(bridge::bridge_absent_specialized_constructor_step);
//     hide(bridge::bridge_absent_connective_step);
//     hide(bridge::bridge_receive_bind_step);
//     hide(bridge::bridge_match_case_step);
//     hide(bridge::bridge_bool_oneof_step);
//     hide(bridge::bridge_expr_atom_step);
//     hide(bridge::bridge_expr_numeric_step);
//     hide(bridge::bridge_expr_big_rational_step);
//     hide(bridge::bridge_expr_fixed_point_step);
//     hide(bridge::bridge_sys_auth_token_step);
//     hide(bridge::bridge_new_empty_collections_step);
//     hide(bridge::bridge_new_one_injection_step);
//     hide(bridge::bridge_new_one_absent_injection_step);
//     hide(bridge::bridge_new_one_uri_step);
//     hide(bridge::bridge_new_collections_step);
//     hide(bridge::bridge_expr_payload_forward_step);
//     hide(bridge::bridge_expr_structural_step);
//     hide(bridge::bridge_evar_step);
//     hide(bridge::bridge_ordered_sequence_step);
//     hide(bridge::bridge_emethod_step);
//     hide(bridge::bridge_emap_finish_step);
//     hide(bridge::bridge_connective_not_step);
//     hide(bridge::bridge_connective_varref_step);
//     hide(bridge::bridge_connective_collection_step);
//     hide(bridge::bridge_unforgeable_bytes_step);
//     hide(bridge::bridge_receive_present_condition_step);
//     hide(bridge::bridge_receive_absent_condition_step);
//     hide(bridge::bridge_nonsemantic_step);
//     hide(epm::linked_indexed_sources);
//     hide(epm::indexed_backing_project);
//     hide(epm::epm_prefix_from_bytes);
//     hide(epm::initial_epm_candidate);
//     hide(epm::indexed_image_wf);
//     hide(epm::frame_layout_wf);
//     hide(epm_source_accept::epm_source_end);
//     hide(epm_source_accept::epm_end_linked);
//     hide(admission::admission_child_summary_agrees);
//     hide(admission::admission_arm_child_summary_agrees);
//     hide(admission::admission_family_admissible);
//     hide(crate::product_epm_source_receipts_wf);
//     hide(crate::product_frame_source_anchors_match);
//     finish_owner_top_shape();
//     let before = finish_owner_top_state();
//     groups::finish_eof_prestate_top_wf(before);
//     bridge_score_finish_epathmap_returns(before);
// }

/// Owner clause, instantiated on the modeled owner-top state: the generic finish exists, it is the
/// group-6 owner finish, equal to the root module's EPathMap-specific score
/// (`finish_owner_top_scored`), and the fold selects it as `Unique`.
pub proof fn finish_owner_top_instance()
    ensures
        groups::finish_eof_prestate(finish_owner_top_state()),
        groups::finish_top_wf(finish_owner_top_state()),
        finish_epathmap_owner(finish_owner_top_state().admission.frames.last()),
        groups::finish_candidate_epathmap_owner(finish_owner_top_state())
            == Some(finish_owner_top_scored()),
        bridge::bridge_score_source_step(finish_owner_top_state(), finish_owner_top_scored()),
        finish_selection(finish_owner_top_state())
            == bridge::BridgeScoreFinishSelection::Unique(finish_owner_top_scored()),
        bridge::bridge_score_finish_selected(finish_owner_top_state())
            == Some(finish_owner_top_scored()),
{
    finish_owner_top_shape();
    finish_owner_top_scores();
    let before = finish_owner_top_state();
    let after = finish_owner_top_scored();
    groups::finish_eof_prestate_top_wf(before);
    groups::finish_source_step_by_group(before, after);
    bridge_score_finish_unique(before, after);
}

/// A one-frame EZipper message (program 32, generated slot count 6) at its EOF, with no field
/// read.
#[verifier::opaque]
pub open spec fn finish_ezipper_top_state() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_initial(groups::finish_witness_root(
        admission::AdmissionFamily::Other, Set::empty(), Seq::empty(), 32, 6,
    ))
}

/// EZipper clause, instantiated on an EZipper that read no field: the group-6 EZipper finish needs
/// slot 0 (the pathmap) and its scored payload return, so the fold ends in `NoCandidate` and no
/// generic finish exists.
pub proof fn finish_ezipper_top_instance()
    ensures
        groups::finish_eof_prestate(finish_ezipper_top_state()),
        groups::finish_top_wf(finish_ezipper_top_state()),
        finish_ezipper_owner(finish_ezipper_top_state().admission.frames.last()),
        finish_selection(finish_ezipper_top_state())
            == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selected(finish_ezipper_top_state()).is_none(),
        forall|after: bridge::ScoreAdmissionBridge|
            !#[trigger] bridge::bridge_score_source_step(finish_ezipper_top_state(), after),
{
    reveal(finish_ezipper_top_state);
    reveal(groups::finish_candidate_ezipper);
    let root = groups::finish_witness_root(
        admission::AdmissionFamily::Other, Set::empty(), Seq::empty(), 32, 6,
    );
    assert forall|slot: int| 0 <= slot < root.slot_count as int
        implies root.required.contains(slot) == (exists|tag: u32|
            admission::admission_required_descriptor_slot(root.program_type_id, tag, slot)) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(32, tag, slot) by {}
    }
    assert(admission::admission_frame_wf(root));
    groups::finish_witness_prestate(root);
    let before = finish_ezipper_top_state();
    groups::finish_eof_prestate_top_wf(before);
    assert(before.admission.frames.last() == root);
    assert(groups::finish_candidate_ezipper(before).is_none());
    bridge_score_finish_epathmap_returns(before);
    bridge_score_finish_never_ambiguous(before);
    assert forall|after: bridge::ScoreAdmissionBridge|
        !#[trigger] bridge::bridge_score_source_step(before, after) by {
        if bridge::bridge_score_source_step(before, after) {
            groups::finish_source_step_routes(before, after);
        }
    }
}

/// EZipper clause, instantiated on the bridge's scored EZipper (the class-6 EZipper witness of
/// part 2a, whose pathmap return carries the owner's score): the group-6 EZipper finish fires and
/// the fold selects it as `Unique`.
pub proof fn finish_ezipper_scored_instance()
    ensures
        groups::finish_eof_prestate(groups::finish_witness_ezipper()),
        finish_ezipper_owner(groups::finish_witness_ezipper().admission.frames.last()),
        groups::finish_candidate_ezipper(groups::finish_witness_ezipper()).is_some(),
        finish_selection(groups::finish_witness_ezipper())
            == bridge::BridgeScoreFinishSelection::Unique(
                groups::finish_candidate_ezipper(groups::finish_witness_ezipper()).unwrap(),
            ),
        bridge::bridge_score_finish_selected(groups::finish_witness_ezipper())
            == groups::finish_candidate_ezipper(groups::finish_witness_ezipper()),
{
    groups::finish_nonvacuous_class_6_ezipper();
    let before = groups::finish_witness_ezipper();
    let after = groups::finish_candidate_ezipper(before).unwrap();
    bridge_score_finish_unique(before, after);
    reveal(groups::finish_candidate_ezipper);
    assert(bridge::bridge_ezipper_score_step(before).is_some());
}

/// The owner, scored (`finish_owner_top_scored`), completed into its Expr parent with the derived
/// payload summary: the admission complete-child step that the root module's
/// `product_epm_modeled_empty_nested_complete_ready` proves enabled.
#[verifier::opaque]
pub open spec fn finish_payload_parent_scored_state() -> bridge::ScoreAdmissionBridge {
    let scored = finish_owner_top_scored();
    bridge::ScoreAdmissionBridge {
        admission: admission::admission_complete_child_step(
            scored.admission,
            scored.admission.frames[1].return_id.unwrap(),
            1,
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: true,
                canonical_fixed_point: true,
                canonical_path_stable: true,
            },
        ).unwrap(),
        transcript: scored.transcript,
    }
}

/// The scored-payload state is a well-formed EOF prestate whose only frame is the modeled Expr
/// parent with one tag-32 `ExprPayload` return that carries a score, so the Expr is complete.
pub proof fn finish_payload_parent_scored_shape()
    ensures
        groups::finish_eof_prestate(finish_payload_parent_scored_state()),
        finish_payload_parent_scored_state().admission.frames.len() == 1,
        finish_epathmap_payload_parent(
            finish_payload_parent_scored_state().admission.frames.last(),
        ),
        admission::admission_family_complete(
            finish_payload_parent_scored_state().admission.frames.last(),
        ),
{
    reveal(finish_payload_parent_scored_state);
    reveal(finish_owner_top_scored);
    reveal(finish_owner_top_state);
    crate::product_epm_modeled_empty_nested_complete_ready();
    finish_owner_top_shape();
    let source = crate::product_epm_modeled_empty_nested_source();
    let product = crate::product_epm_nested_source_witness_after(source);
    assert(product.bridge == finish_owner_top_state());
    crate::product_epm_source_bridge_finish_selected_exact(product);
    let scored = crate::product_epm_source_bridge_finish_selected(product).unwrap().0;
    assert(scored == finish_owner_top_scored());
    assert(bridge::bridge_score_step_shape(product.bridge, scored));
    assert(bridge::bridge_well_formed(scored));
    assert(admission::admission_state_wf(scored.admission));
    assert(scored.admission.frames.len() == 2);
    assert(scored.admission.frames[0] == source.admission.frames[0]);
    let summary = admission::AdmissionReturnedKind::ExprPayload {
        score_present: true,
        canonical_fixed_point: true,
        canonical_path_stable: true,
    };
    let returned_id = scored.admission.frames[1].return_id.unwrap();
    admission::admission_complete_child_preserves_wf(scored.admission, returned_id, 1, summary);
    let completed = admission::admission_complete_child_step(
        scored.admission, returned_id, 1, summary,
    ).unwrap();
    let state = finish_payload_parent_scored_state();
    assert(state.admission == completed);
    assert(state.transcript == scored.transcript);
    assert(admission::admission_state_wf(completed));
    assert(completed.arena == scored.admission.arena);
    assert(bridge::bridge_well_formed(state));
    let outer = source.admission.frames[0];
    assert(outer.family == admission::AdmissionFamily::Expr);
    assert(outer.scalar_occurrences.len() == 0);
    assert(outer.returns.len() == 0);
    assert(outer.waiting_slot == Some(0usize));
    assert(outer.waiting_tag == Some(32u32));
    assert(completed.frames.len() == 1);
    let top = completed.frames[0];
    assert(top == completed.frames.last());
    assert(top.family == admission::AdmissionFamily::Expr);
    assert(top.scalar_occurrences.len() == 0);
    assert(top.returns.len() == 1);
    assert(top.returns[0].slot == 0);
    assert(top.returns[0].tag == 32);
    assert(top.returns[0].kind == summary);
    assert(admission::admission_expr_has_score(top));
    assert(admission::admission_family_complete(top));
    assert(top.cursor == top.region_end);
    assert(top.waiting_id.is_none());
    assert(top.nested_started == top.nested_resolved);
    assert(top.score_handle.is_none());
    reveal(groups::finish_eof_prestate);
}

/// Complete payload-parent clause, instantiated on the owner's scored payload completed into the
/// modeled Expr parent: the fold adds exactly the payload forward candidate.
pub proof fn finish_payload_parent_complete_instance()
    ensures
        groups::finish_eof_prestate(finish_payload_parent_scored_state()),
        groups::finish_top_wf(finish_payload_parent_scored_state()),
        finish_epathmap_payload_parent(
            finish_payload_parent_scored_state().admission.frames.last(),
        ),
        admission::admission_family_complete(
            finish_payload_parent_scored_state().admission.frames.last(),
        ),
        finish_selection(finish_payload_parent_scored_state())
            == bridge::bridge_add_score_finish_candidate(
                bridge::BridgeScoreFinishSelection::NoCandidate,
                groups::finish_candidate_expr_payload_forward(
                    finish_payload_parent_scored_state(),
                ),
            ),
        finish_selection(finish_payload_parent_scored_state())
            != bridge::BridgeScoreFinishSelection::Ambiguous,
{
    finish_payload_parent_scored_shape();
    let before = finish_payload_parent_scored_state();
    groups::finish_eof_prestate_top_wf(before);
    bridge_score_finish_epathmap_returns(before);
}

/// The tag-32 `ExprPayload` summary at slot 0, without a score handle.
pub open spec fn finish_payload_return(score_present: bool)
    -> admission::AdmissionReturnedSummary {
    admission::AdmissionReturnedSummary {
        slot: 0,
        tag: 32,
        body_id: 1,
        body_bytes: Seq::empty(),
        score_handle: None,
        score_origin_body_id: None,
        kind: admission::AdmissionReturnedKind::ExprPayload {
            score_present,
            canonical_fixed_point: true,
            canonical_path_stable: true,
        },
    }
}

/// A one-frame Expr (program 24) whose slot 0 holds one tag-32 `ExprPayload` return without a
/// score: slot 0 is seen, not scalar, and has no score, so the Expr is incomplete.
#[verifier::opaque]
pub open spec fn finish_payload_parent_unscored_state() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_initial(groups::finish_witness_root(
        admission::AdmissionFamily::Expr, Set::empty().insert(0),
        seq![finish_payload_return(false)], 24, 1,
    ))
}

/// Incomplete payload-parent clause, instantiated: the fold adds exactly the nonsemantic
/// candidate, which fires there, so the fold is `Unique`.
pub proof fn finish_payload_parent_incomplete_instance()
    ensures
        groups::finish_eof_prestate(finish_payload_parent_unscored_state()),
        groups::finish_top_wf(finish_payload_parent_unscored_state()),
        finish_epathmap_payload_parent(
            finish_payload_parent_unscored_state().admission.frames.last(),
        ),
        !admission::admission_family_complete(
            finish_payload_parent_unscored_state().admission.frames.last(),
        ),
        finish_selection(finish_payload_parent_unscored_state())
            == bridge::bridge_add_score_finish_candidate(
                bridge::BridgeScoreFinishSelection::NoCandidate,
                groups::finish_candidate_nonsemantic(finish_payload_parent_unscored_state()),
            ),
        groups::finish_candidate_nonsemantic(finish_payload_parent_unscored_state()).is_some(),
        finish_selection(finish_payload_parent_unscored_state())
            == bridge::BridgeScoreFinishSelection::Unique(
                groups::finish_candidate_nonsemantic(
                    finish_payload_parent_unscored_state(),
                ).unwrap(),
            ),
{
    reveal(finish_payload_parent_unscored_state);
    reveal(groups::finish_candidate_nonsemantic);
    let root = groups::finish_witness_root(
        admission::AdmissionFamily::Expr, Set::empty().insert(0),
        seq![finish_payload_return(false)], 24, 1,
    );
    assert forall|slot: int| 0 <= slot < root.slot_count as int
        implies root.required.contains(slot) == (exists|tag: u32|
            admission::admission_required_descriptor_slot(root.program_type_id, tag, slot)) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(24, tag, slot) by {}
    }
    assert(admission::admission_frame_wf(root));
    groups::finish_witness_prestate(root);
    let before = finish_payload_parent_unscored_state();
    groups::finish_eof_prestate_top_wf(before);
    assert(!admission::admission_expr_has_score(root));
    assert(!admission::admission_family_complete(root));
    groups::finish_witness_leaf_fires(root, i32::MAX);
    assert(groups::finish_candidate_nonsemantic(before).is_some());
    bridge_score_finish_epathmap_returns(before);
}

/// A one-frame Par message (program 1, generated slot count 12) at its EOF, the shape of a
/// ParValue or RawParEscape child of an EPathMap entry.
#[verifier::opaque]
pub open spec fn finish_value_child_state() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_initial(groups::finish_witness_root(
        admission::AdmissionFamily::Other, Set::empty(), Seq::empty(), 1, 12,
    ))
}

/// Value-child clause, instantiated: the fold adds exactly the raw Par candidate.
pub proof fn finish_value_child_instance()
    ensures
        groups::finish_eof_prestate(finish_value_child_state()),
        groups::finish_top_wf(finish_value_child_state()),
        finish_epathmap_value_child(finish_value_child_state().admission.frames.last()),
        finish_selection(finish_value_child_state())
            == bridge::bridge_add_score_finish_candidate(
                bridge::BridgeScoreFinishSelection::NoCandidate,
                groups::finish_candidate_raw_par(finish_value_child_state()),
            ),
        finish_selection(finish_value_child_state())
            != bridge::BridgeScoreFinishSelection::Ambiguous,
{
    reveal(finish_value_child_state);
    let root = groups::finish_witness_root(
        admission::AdmissionFamily::Other, Set::empty(), Seq::empty(), 1, 12,
    );
    assert forall|slot: int| 0 <= slot < root.slot_count as int
        implies root.required.contains(slot) == (exists|tag: u32|
            admission::admission_required_descriptor_slot(root.program_type_id, tag, slot)) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(1, tag, slot) by {}
    }
    assert(admission::admission_frame_wf(root));
    groups::finish_witness_prestate(root);
    let before = finish_value_child_state();
    groups::finish_eof_prestate_top_wf(before);
    assert(before.admission.frames.last() == root);
    bridge_score_finish_epathmap_returns(before);
}

}
