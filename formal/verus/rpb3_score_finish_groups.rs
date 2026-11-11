//! Open obligation 2 of `docs/design/pathmap/rpb3-admission-source-traceability.md`, part
//! 2a: each of the six opaque score-finish groups folded by `bridge_score_finish_selected`
//! covers the finishing well-formed EOF prestates of its class and is disjoint from the
//! other groups over every well-formed EOF prestate.
//!
//! `finish_route` classifies the top frame by family, program id and presence bits and
//! returns the fold index (1..43) of the only generic score candidate that can fire on it,
//! or 0 when no candidate can. Each `finish_routes_*` lemma proves that its candidate fires
//! only at its own index, so no two candidates fire on one prestate. Class K is the index
//! range of group K, and `finish_routed_group_K` is the candidate of group K that the route
//! selects:
//! - group 1 (1..8): raw Par and the cost frames;
//! - group 2 (9..16): complete Bundle, If, Send and Match, absent oneofs, receive binds and
//!   match cases;
//! - group 3 (17..24): scalar Expr and Connective arms, BigRational, FixedPoint, the system
//!   auth token, New without URIs and with at most one injection;
//! - group 4 (25..33): the other New shapes, Expr payload forwarding, structural Expr
//!   messages, EVar, ordered collections, EMethod, EMap;
//! - group 5 (34..40): Connective message arms, unforgeable bytes, complete Receive, and the
//!   nonsemantic finish of an incomplete component;
//! - group 6 (41..43): Var (program 6), the EPathMap owner (program 31, production
//!   `reduce_epathmap`) and EZipper (program 32, production `reduce_ezipper`), both scored
//!   like production's `rpb3_pathmap_score`.
//!
//! Disjointness needs only `finish_top_wf`. Coverage also needs a successful finish,
//! `bridge_score_source_step(before, after)`: `finish_coverage_counterexample` is a
//! well-formed EOF prestate without one. Every class has a concrete witness
//! (`finish_nonvacuous_*`).
//!
//! The candidate wrappers, `finish_top_wf`, `finish_route` and the witness states are
//! opaque. Only the routing lemmas, the group bodies, `finish_source_step_by_group` and the
//! witness lemmas reveal them (the sources lemmas reveal only `finish_top_wf`), so the folds
//! and the composition lemmas never unfold a candidate step.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_runtime_frame_source_refinement as runtime_source,
    rpb3_score_admission_bridge as bridge, rpb3_score_transcript as score,
};

verus! {

/// The top frame exists and is well formed. Disjointness needs nothing else.
#[verifier::opaque]
pub open spec fn finish_top_wf(before: bridge::ScoreAdmissionBridge) -> bool {
    &&& before.admission.frames.len() > 0
    &&& admission::admission_frame_wf(before.admission.frames.last())
}

/// A well-formed EOF prestate: a checked score bridge and a well-formed admission state whose top
/// frame consumed its region, waits for no child, has no open nested obligation and no score yet.
#[verifier::opaque]
pub open spec fn finish_eof_prestate(before: bridge::ScoreAdmissionBridge) -> bool {
    &&& bridge::bridge_well_formed(before)
    &&& admission::admission_state_wf(before.admission)
    &&& before.admission.frames.last().cursor == before.admission.frames.last().region_end
    &&& before.admission.frames.last().waiting_id.is_none()
    &&& before.admission.frames.last().nested_started
        == before.admission.frames.last().nested_resolved
    &&& before.admission.frames.last().score_handle.is_none()
}

pub open spec fn finish_route_other(top: admission::AdmissionFrameState) -> int {
    let program = top.program_type_id;
    if program == 1 {
        1
    } else if program == 10 {
        if !top.seen.contains(0) {
            2
        } else if top.scalar_slots.contains(0) {
            3
        } else {
            5
        }
    } else if program == 11 {
        4
    } else if 36 <= program && program <= 54 {
        29
    } else if 27 <= program && program <= 29 {
        31
    } else if program == 33 {
        32
    } else if program == 30 {
        33
    } else if program == 6 {
        41
    } else if program == 31 {
        42
    } else if program == 32 {
        43
    } else {
        0
    }
}

pub open spec fn finish_route_expr(top: admission::AdmissionFrameState) -> int {
    if !admission::admission_family_complete(top) {
        40
    } else if !top.seen.contains(0) {
        13
    } else if top.scalar_occurrences.len() == 1 {
        let tag = top.scalar_occurrences[0].tag;
        if tag == 1 {
            17
        } else if tag == 3 || tag == 4 || tag == 25 || tag == 35 {
            18
        } else if tag == 2 || tag == 34 {
            19
        } else {
            0
        }
    } else if top.returns.len() == 1 {
        match top.returns[0].kind {
            admission::AdmissionReturnedKind::BigRational { .. } => 20,
            admission::AdmissionReturnedKind::FixedPoint { .. } => 21,
            admission::AdmissionReturnedKind::ExprPayload { .. } => 28,
            _ => 0,
        }
    } else {
        0
    }
}

pub open spec fn finish_route_unforgeable(top: admission::AdmissionFrameState) -> int {
    if !top.seen.contains(0) {
        13
    } else if top.returns.len() == 1 && top.returns[0].tag == 4 {
        22
    } else {
        37
    }
}

pub open spec fn finish_route_connective(top: admission::AdmissionFrameState) -> int {
    if !top.seen.contains(0) {
        14
    } else if top.scalar_occurrences.len() == 1 {
        17
    } else if top.returns.len() == 1 {
        if top.returns[0].tag == 3 {
            34
        } else if top.returns[0].tag == 4 {
            35
        } else {
            36
        }
    } else {
        0
    }
}

pub open spec fn finish_route_new(top: admission::AdmissionFrameState) -> int {
    if !admission::admission_family_complete(top) {
        40
    } else if !top.seen.contains(2) {
        if top.returns.len() == 1 {
            23
        } else if top.returns.len() == 2 {
            match top.returns[1].kind {
                admission::AdmissionReturnedKind::Map { value_score_handle: Some(_), .. } => 24,
                _ => 25,
            }
        } else {
            27
        }
    } else {
        match bridge::bridge_string_bytes_for_slot(top.scalar_occurrences, 2, 3, 15u8) {
            Some(uris) => if uris.len() == 1 && top.returns.len() == 1 {
                26
            } else {
                27
            },
            None => 27,
        }
    }
}

pub open spec fn finish_route_receive(top: admission::AdmissionFrameState) -> int {
    if !admission::admission_family_complete(top) {
        40
    } else if top.seen.contains(7) {
        38
    } else {
        39
    }
}

/// Fold index of the only generic score candidate that can fire on `top` (0: none can).
#[verifier::opaque]
pub open spec fn finish_route(top: admission::AdmissionFrameState) -> int {
    let complete = admission::admission_family_complete(top);
    match top.family {
        admission::AdmissionFamily::Other => finish_route_other(top),
        admission::AdmissionFamily::CostSignatureFrame => 6,
        admission::AdmissionFamily::CostRegionFrame => 7,
        admission::AdmissionFamily::CostTokenStack => 8,
        admission::AdmissionFamily::Bundle => if complete { 9 } else { 40 },
        admission::AdmissionFamily::If => if complete { 10 } else { 40 },
        admission::AdmissionFamily::Send => if complete { 11 } else { 40 },
        admission::AdmissionFamily::Match => if complete { 12 } else { 40 },
        admission::AdmissionFamily::ReceiveBindFrame => if complete { 15 } else { 0 },
        admission::AdmissionFamily::MatchCaseFrame => if complete { 16 } else { 0 },
        admission::AdmissionFamily::EVarFrame => if complete { 30 } else { 0 },
        admission::AdmissionFamily::BigRationalFrame => 0,
        admission::AdmissionFamily::FixedPointFrame => 0,
        admission::AdmissionFamily::Expr => finish_route_expr(top),
        admission::AdmissionFamily::GUnforgeable => finish_route_unforgeable(top),
        admission::AdmissionFamily::Connective => finish_route_connective(top),
        admission::AdmissionFamily::New => finish_route_new(top),
        admission::AdmissionFamily::Receive => finish_route_receive(top),
    }
}

pub open spec fn finish_route_of(before: bridge::ScoreAdmissionBridge) -> int {
    finish_route(before.admission.frames.last())
}

/// Group 1: raw Par and cost frames (fold indices 1..8).
pub open spec fn finish_class_1(before: bridge::ScoreAdmissionBridge) -> bool {
    &&& before.admission.frames.len() > 0
    &&& 1 <= finish_route_of(before) && finish_route_of(before) <= 8
}

/// Group 2: complete primary components, absent oneofs, receive binds, match cases (9..16).
pub open spec fn finish_class_2(before: bridge::ScoreAdmissionBridge) -> bool {
    &&& before.admission.frames.len() > 0
    &&& 9 <= finish_route_of(before) && finish_route_of(before) <= 16
}

/// Group 3: scalar arms, BigRational, FixedPoint, the system auth token, small New frames (17..24).
pub open spec fn finish_class_3(before: bridge::ScoreAdmissionBridge) -> bool {
    &&& before.admission.frames.len() > 0
    &&& 17 <= finish_route_of(before) && finish_route_of(before) <= 24
}

/// Group 4: other New frames, payload forwarding, structural Expr, EVar, EMethod, EMap (25..33).
pub open spec fn finish_class_4(before: bridge::ScoreAdmissionBridge) -> bool {
    &&& before.admission.frames.len() > 0
    &&& 25 <= finish_route_of(before) && finish_route_of(before) <= 33
}

/// Group 5: Connective arms, unforgeable bytes, complete Receive, nonsemantic finish (34..40).
pub open spec fn finish_class_5(before: bridge::ScoreAdmissionBridge) -> bool {
    &&& before.admission.frames.len() > 0
    &&& 34 <= finish_route_of(before) && finish_route_of(before) <= 40
}

/// Group 6: Var, the EPathMap owner and EZipper messages (41..43).
pub open spec fn finish_class_6(before: bridge::ScoreAdmissionBridge) -> bool {
    &&& before.admission.frames.len() > 0
    &&& 41 <= finish_route_of(before) && finish_route_of(before) <= 43
}

/// A candidate exactly as the group bodies extract it from its step; opaque, so that only the
/// lemmas that need the step see it.
#[verifier::opaque]
pub open spec fn finish_candidate_raw_par(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_raw_par_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_cost_atom_absent(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_cost_atom_absent_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_cost_atom_bytes(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_cost_atom_bytes_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_cost_bound(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_cost_bound_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_cost_atom_bound_forward(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_cost_atom_bound_forward_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_cost_signature(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_cost_signature_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_cost_region(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_cost_region_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_cost_token_stack(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_cost_token_stack_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_bundle(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_bundle_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_if(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_if_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_send(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_send_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_match_component(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_match_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_absent_specialized(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_absent_specialized_constructor_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_absent_connective(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_absent_connective_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_receive_bind(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_receive_bind_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_match_case(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_match_case_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_bool_oneof(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_bool_oneof_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_expr_atom(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_expr_atom_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_expr_numeric(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_expr_numeric_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_expr_big_rational(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_expr_big_rational_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_expr_fixed_point(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_expr_fixed_point_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_sys_auth_token(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_sys_auth_token_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_new_empty_collections(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_new_empty_collections_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_new_one_injection(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_new_one_injection_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_new_one_absent_injection(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_new_one_absent_injection_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_new_one_uri(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_new_one_uri_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_new_collections(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_new_collections_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_expr_payload_forward(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_expr_payload_forward_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_expr_structural(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_expr_structural_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_evar(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_evar_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_ordered_sequence(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_ordered_sequence_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_emethod(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_emethod_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_emap_finish(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_emap_finish_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_connective_not(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_connective_not_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_connective_varref(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_connective_varref_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_connective_collection(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_connective_collection_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_unforgeable_bytes(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_unforgeable_bytes_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_receive_present_condition(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_receive_present_condition_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_receive_absent_condition(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_receive_absent_condition_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_nonsemantic(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_nonsemantic_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_var(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_var_score_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_epathmap_owner(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_epathmap_owner_score_step(before) {
        None => None,
        Some(next) => Some(next),
    }
}

#[verifier::opaque]
pub open spec fn finish_candidate_ezipper(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_ezipper_score_step(before) {
        None => None,
        Some(next) => Some(next),
    }
}

/// The candidate of group K that the route selects (None outside class K).
pub open spec fn finish_routed_group_1(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    let route = finish_route_of(before);
    if route == 1 {
        finish_candidate_raw_par(before)
    } else if route == 2 {
        finish_candidate_cost_atom_absent(before)
    } else if route == 3 {
        finish_candidate_cost_atom_bytes(before)
    } else if route == 4 {
        finish_candidate_cost_bound(before)
    } else if route == 5 {
        finish_candidate_cost_atom_bound_forward(before)
    } else if route == 6 {
        finish_candidate_cost_signature(before)
    } else if route == 7 {
        finish_candidate_cost_region(before)
    } else if route == 8 {
        finish_candidate_cost_token_stack(before)
    } else {
        None
    }
}

pub open spec fn finish_routed_group_2(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    let route = finish_route_of(before);
    if route == 9 {
        finish_candidate_bundle(before)
    } else if route == 10 {
        finish_candidate_if(before)
    } else if route == 11 {
        finish_candidate_send(before)
    } else if route == 12 {
        finish_candidate_match_component(before)
    } else if route == 13 {
        finish_candidate_absent_specialized(before)
    } else if route == 14 {
        finish_candidate_absent_connective(before)
    } else if route == 15 {
        finish_candidate_receive_bind(before)
    } else if route == 16 {
        finish_candidate_match_case(before)
    } else {
        None
    }
}

pub open spec fn finish_routed_group_3(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    let route = finish_route_of(before);
    if route == 17 {
        finish_candidate_bool_oneof(before)
    } else if route == 18 {
        finish_candidate_expr_atom(before)
    } else if route == 19 {
        finish_candidate_expr_numeric(before)
    } else if route == 20 {
        finish_candidate_expr_big_rational(before)
    } else if route == 21 {
        finish_candidate_expr_fixed_point(before)
    } else if route == 22 {
        finish_candidate_sys_auth_token(before)
    } else if route == 23 {
        finish_candidate_new_empty_collections(before)
    } else if route == 24 {
        finish_candidate_new_one_injection(before)
    } else {
        None
    }
}

pub open spec fn finish_routed_group_4(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    let route = finish_route_of(before);
    if route == 25 {
        finish_candidate_new_one_absent_injection(before)
    } else if route == 26 {
        finish_candidate_new_one_uri(before)
    } else if route == 27 {
        finish_candidate_new_collections(before)
    } else if route == 28 {
        finish_candidate_expr_payload_forward(before)
    } else if route == 29 {
        finish_candidate_expr_structural(before)
    } else if route == 30 {
        finish_candidate_evar(before)
    } else if route == 31 {
        finish_candidate_ordered_sequence(before)
    } else if route == 32 {
        finish_candidate_emethod(before)
    } else if route == 33 {
        finish_candidate_emap_finish(before)
    } else {
        None
    }
}

pub open spec fn finish_routed_group_5(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    let route = finish_route_of(before);
    if route == 34 {
        finish_candidate_connective_not(before)
    } else if route == 35 {
        finish_candidate_connective_varref(before)
    } else if route == 36 {
        finish_candidate_connective_collection(before)
    } else if route == 37 {
        finish_candidate_unforgeable_bytes(before)
    } else if route == 38 {
        finish_candidate_receive_present_condition(before)
    } else if route == 39 {
        finish_candidate_receive_absent_condition(before)
    } else if route == 40 {
        finish_candidate_nonsemantic(before)
    } else {
        None
    }
}

pub open spec fn finish_routed_group_6(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    let route = finish_route_of(before);
    if route == 41 {
        finish_candidate_var(before)
    } else if route == 42 {
        finish_candidate_epathmap_owner(before)
    } else if route == 43 {
        finish_candidate_ezipper(before)
    } else {
        None
    }
}

/// The candidate chosen by the route, whichever group owns it.
pub open spec fn finish_routed(before: bridge::ScoreAdmissionBridge)
    -> Option<bridge::ScoreAdmissionBridge> {
    if finish_class_1(before) {
        finish_routed_group_1(before)
    } else if finish_class_2(before) {
        finish_routed_group_2(before)
    } else if finish_class_3(before) {
        finish_routed_group_3(before)
    } else if finish_class_4(before) {
        finish_routed_group_4(before)
    } else if finish_class_5(before) {
        finish_routed_group_5(before)
    } else if finish_class_6(before) {
        finish_routed_group_6(before)
    } else {
        None
    }
}

pub open spec fn finish_candidate_agrees(
    candidate: Option<bridge::ScoreAdmissionBridge>, after: bridge::ScoreAdmissionBridge,
) -> bool {
    match candidate {
        None => false,
        Some(next) => bridge::bridge_same(next, after),
    }
}

/// The disjuncts of `bridge_score_source_step` contributed by group 1.
pub open spec fn finish_group_1_source(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
) -> bool {
    ||| finish_candidate_agrees(finish_candidate_raw_par(before), after)
    ||| finish_candidate_agrees(finish_candidate_cost_atom_absent(before), after)
    ||| finish_candidate_agrees(finish_candidate_cost_atom_bytes(before), after)
    ||| finish_candidate_agrees(finish_candidate_cost_bound(before), after)
    ||| finish_candidate_agrees(finish_candidate_cost_atom_bound_forward(before), after)
    ||| finish_candidate_agrees(finish_candidate_cost_signature(before), after)
    ||| finish_candidate_agrees(finish_candidate_cost_region(before), after)
    ||| finish_candidate_agrees(finish_candidate_cost_token_stack(before), after)
}

pub open spec fn finish_group_2_source(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
) -> bool {
    ||| finish_candidate_agrees(finish_candidate_bundle(before), after)
    ||| finish_candidate_agrees(finish_candidate_if(before), after)
    ||| finish_candidate_agrees(finish_candidate_send(before), after)
    ||| finish_candidate_agrees(finish_candidate_match_component(before), after)
    ||| finish_candidate_agrees(finish_candidate_absent_specialized(before), after)
    ||| finish_candidate_agrees(finish_candidate_absent_connective(before), after)
    ||| finish_candidate_agrees(finish_candidate_receive_bind(before), after)
    ||| finish_candidate_agrees(finish_candidate_match_case(before), after)
}

pub open spec fn finish_group_3_source(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
) -> bool {
    ||| finish_candidate_agrees(finish_candidate_bool_oneof(before), after)
    ||| finish_candidate_agrees(finish_candidate_expr_atom(before), after)
    ||| finish_candidate_agrees(finish_candidate_expr_numeric(before), after)
    ||| finish_candidate_agrees(finish_candidate_expr_big_rational(before), after)
    ||| finish_candidate_agrees(finish_candidate_expr_fixed_point(before), after)
    ||| finish_candidate_agrees(finish_candidate_sys_auth_token(before), after)
    ||| finish_candidate_agrees(finish_candidate_new_empty_collections(before), after)
    ||| finish_candidate_agrees(finish_candidate_new_one_injection(before), after)
}

pub open spec fn finish_group_4_source(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
) -> bool {
    ||| finish_candidate_agrees(finish_candidate_new_one_absent_injection(before), after)
    ||| finish_candidate_agrees(finish_candidate_new_one_uri(before), after)
    ||| finish_candidate_agrees(finish_candidate_new_collections(before), after)
    ||| finish_candidate_agrees(finish_candidate_expr_payload_forward(before), after)
    ||| finish_candidate_agrees(finish_candidate_expr_structural(before), after)
    ||| finish_candidate_agrees(finish_candidate_evar(before), after)
    ||| finish_candidate_agrees(finish_candidate_ordered_sequence(before), after)
    ||| finish_candidate_agrees(finish_candidate_emethod(before), after)
    ||| finish_candidate_agrees(finish_candidate_emap_finish(before), after)
}

pub open spec fn finish_group_5_source(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
) -> bool {
    ||| finish_candidate_agrees(finish_candidate_connective_not(before), after)
    ||| finish_candidate_agrees(finish_candidate_connective_varref(before), after)
    ||| finish_candidate_agrees(finish_candidate_connective_collection(before), after)
    ||| finish_candidate_agrees(finish_candidate_unforgeable_bytes(before), after)
    ||| finish_candidate_agrees(finish_candidate_receive_present_condition(before), after)
    ||| finish_candidate_agrees(finish_candidate_receive_absent_condition(before), after)
    ||| finish_candidate_agrees(finish_candidate_nonsemantic(before), after)
}

pub open spec fn finish_group_6_source(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
) -> bool {
    ||| finish_candidate_agrees(finish_candidate_var(before), after)
    ||| finish_candidate_agrees(finish_candidate_epathmap_owner(before), after)
    ||| finish_candidate_agrees(finish_candidate_ezipper(before), after)
}

/// Group K's fold, with each candidate named by its wrapper.
pub open spec fn finish_group_1_adds(
    before: bridge::ScoreAdmissionBridge, selection_0: bridge::BridgeScoreFinishSelection,
) -> bridge::BridgeScoreFinishSelection {
    let selection_1 = bridge::bridge_add_score_finish_candidate(
        selection_0, finish_candidate_raw_par(before),
    );
    let selection_2 = bridge::bridge_add_score_finish_candidate(
        selection_1, finish_candidate_cost_atom_absent(before),
    );
    let selection_3 = bridge::bridge_add_score_finish_candidate(
        selection_2, finish_candidate_cost_atom_bytes(before),
    );
    let selection_4 = bridge::bridge_add_score_finish_candidate(
        selection_3, finish_candidate_cost_bound(before),
    );
    let selection_5 = bridge::bridge_add_score_finish_candidate(
        selection_4, finish_candidate_cost_atom_bound_forward(before),
    );
    let selection_6 = bridge::bridge_add_score_finish_candidate(
        selection_5, finish_candidate_cost_signature(before),
    );
    let selection_7 = bridge::bridge_add_score_finish_candidate(
        selection_6, finish_candidate_cost_region(before),
    );
    let selection_8 = bridge::bridge_add_score_finish_candidate(
        selection_7, finish_candidate_cost_token_stack(before),
    );
    selection_8
}

pub open spec fn finish_group_2_adds(
    before: bridge::ScoreAdmissionBridge, selection_0: bridge::BridgeScoreFinishSelection,
) -> bridge::BridgeScoreFinishSelection {
    let selection_1 = bridge::bridge_add_score_finish_candidate(
        selection_0, finish_candidate_bundle(before),
    );
    let selection_2 = bridge::bridge_add_score_finish_candidate(
        selection_1, finish_candidate_if(before),
    );
    let selection_3 = bridge::bridge_add_score_finish_candidate(
        selection_2, finish_candidate_send(before),
    );
    let selection_4 = bridge::bridge_add_score_finish_candidate(
        selection_3, finish_candidate_match_component(before),
    );
    let selection_5 = bridge::bridge_add_score_finish_candidate(
        selection_4, finish_candidate_absent_specialized(before),
    );
    let selection_6 = bridge::bridge_add_score_finish_candidate(
        selection_5, finish_candidate_absent_connective(before),
    );
    let selection_7 = bridge::bridge_add_score_finish_candidate(
        selection_6, finish_candidate_receive_bind(before),
    );
    let selection_8 = bridge::bridge_add_score_finish_candidate(
        selection_7, finish_candidate_match_case(before),
    );
    selection_8
}

pub open spec fn finish_group_3_adds(
    before: bridge::ScoreAdmissionBridge, selection_0: bridge::BridgeScoreFinishSelection,
) -> bridge::BridgeScoreFinishSelection {
    let selection_1 = bridge::bridge_add_score_finish_candidate(
        selection_0, finish_candidate_bool_oneof(before),
    );
    let selection_2 = bridge::bridge_add_score_finish_candidate(
        selection_1, finish_candidate_expr_atom(before),
    );
    let selection_3 = bridge::bridge_add_score_finish_candidate(
        selection_2, finish_candidate_expr_numeric(before),
    );
    let selection_4 = bridge::bridge_add_score_finish_candidate(
        selection_3, finish_candidate_expr_big_rational(before),
    );
    let selection_5 = bridge::bridge_add_score_finish_candidate(
        selection_4, finish_candidate_expr_fixed_point(before),
    );
    let selection_6 = bridge::bridge_add_score_finish_candidate(
        selection_5, finish_candidate_sys_auth_token(before),
    );
    let selection_7 = bridge::bridge_add_score_finish_candidate(
        selection_6, finish_candidate_new_empty_collections(before),
    );
    let selection_8 = bridge::bridge_add_score_finish_candidate(
        selection_7, finish_candidate_new_one_injection(before),
    );
    selection_8
}

pub open spec fn finish_group_4_adds(
    before: bridge::ScoreAdmissionBridge, selection_0: bridge::BridgeScoreFinishSelection,
) -> bridge::BridgeScoreFinishSelection {
    let selection_1 = bridge::bridge_add_score_finish_candidate(
        selection_0, finish_candidate_new_one_absent_injection(before),
    );
    let selection_2 = bridge::bridge_add_score_finish_candidate(
        selection_1, finish_candidate_new_one_uri(before),
    );
    let selection_3 = bridge::bridge_add_score_finish_candidate(
        selection_2, finish_candidate_new_collections(before),
    );
    let selection_4 = bridge::bridge_add_score_finish_candidate(
        selection_3, finish_candidate_expr_payload_forward(before),
    );
    let selection_5 = bridge::bridge_add_score_finish_candidate(
        selection_4, finish_candidate_expr_structural(before),
    );
    let selection_6 = bridge::bridge_add_score_finish_candidate(
        selection_5, finish_candidate_evar(before),
    );
    let selection_7 = bridge::bridge_add_score_finish_candidate(
        selection_6, finish_candidate_ordered_sequence(before),
    );
    let selection_8 = bridge::bridge_add_score_finish_candidate(
        selection_7, finish_candidate_emethod(before),
    );
    let selection_9 = bridge::bridge_add_score_finish_candidate(
        selection_8, finish_candidate_emap_finish(before),
    );
    selection_9
}

pub open spec fn finish_group_5_adds(
    before: bridge::ScoreAdmissionBridge, selection_0: bridge::BridgeScoreFinishSelection,
) -> bridge::BridgeScoreFinishSelection {
    let selection_1 = bridge::bridge_add_score_finish_candidate(
        selection_0, finish_candidate_connective_not(before),
    );
    let selection_2 = bridge::bridge_add_score_finish_candidate(
        selection_1, finish_candidate_connective_varref(before),
    );
    let selection_3 = bridge::bridge_add_score_finish_candidate(
        selection_2, finish_candidate_connective_collection(before),
    );
    let selection_4 = bridge::bridge_add_score_finish_candidate(
        selection_3, finish_candidate_unforgeable_bytes(before),
    );
    let selection_5 = bridge::bridge_add_score_finish_candidate(
        selection_4, finish_candidate_receive_present_condition(before),
    );
    let selection_6 = bridge::bridge_add_score_finish_candidate(
        selection_5, finish_candidate_receive_absent_condition(before),
    );
    let selection_7 = bridge::bridge_add_score_finish_candidate(
        selection_6, finish_candidate_nonsemantic(before),
    );
    selection_7
}

pub open spec fn finish_group_6_adds(
    before: bridge::ScoreAdmissionBridge, selection_0: bridge::BridgeScoreFinishSelection,
) -> bridge::BridgeScoreFinishSelection {
    let selection_1 = bridge::bridge_add_score_finish_candidate(
        selection_0, finish_candidate_var(before),
    );
    let selection_2 = bridge::bridge_add_score_finish_candidate(
        selection_1, finish_candidate_epathmap_owner(before),
    );
    let selection_3 = bridge::bridge_add_score_finish_candidate(
        selection_2, finish_candidate_ezipper(before),
    );
    selection_3
}

pub proof fn finish_eof_prestate_top_wf(before: bridge::ScoreAdmissionBridge)
    requires finish_eof_prestate(before),
    ensures
        finish_top_wf(before),
        bridge::bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
{
    reveal(finish_eof_prestate);
    reveal(finish_top_wf);
    assert(admission::admission_frame_wf(
        before.admission.frames[before.admission.frames.len() - 1],
    ));
}

/// Routing: the candidate fires only when the route equals its fold index.
pub proof fn finish_routes_raw_par(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_raw_par(before).is_some() ==> finish_route_of(before) == 1,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_raw_par);
    reveal(finish_route);
}

pub proof fn finish_routes_cost_atom_absent(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_cost_atom_absent(before).is_some() ==> finish_route_of(before) == 2,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_cost_atom_absent);
    reveal(finish_route);
}

pub proof fn finish_routes_cost_atom_bytes(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_cost_atom_bytes(before).is_some() ==> finish_route_of(before) == 3,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_cost_atom_bytes);
    reveal(finish_route);
}

pub proof fn finish_routes_cost_bound(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_cost_bound(before).is_some() ==> finish_route_of(before) == 4,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_cost_bound);
    reveal(finish_route);
}

pub proof fn finish_routes_cost_atom_bound_forward(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_cost_atom_bound_forward(before).is_some()
        ==> finish_route_of(before) == 5,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_cost_atom_bound_forward);
    reveal(finish_route);
}

pub proof fn finish_routes_cost_signature(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_cost_signature(before).is_some() ==> finish_route_of(before) == 6,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_cost_signature);
    reveal(finish_route);
}

pub proof fn finish_routes_cost_region(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_cost_region(before).is_some() ==> finish_route_of(before) == 7,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_cost_region);
    reveal(finish_route);
}

pub proof fn finish_routes_cost_token_stack(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_cost_token_stack(before).is_some() ==> finish_route_of(before) == 8,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_cost_token_stack);
    reveal(finish_route);
}

pub proof fn finish_routes_bundle(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_bundle(before).is_some() ==> finish_route_of(before) == 9,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_bundle);
    reveal(finish_route);
}

pub proof fn finish_routes_if(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_if(before).is_some() ==> finish_route_of(before) == 10,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_if);
    reveal(finish_route);
}

pub proof fn finish_routes_send(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_send(before).is_some() ==> finish_route_of(before) == 11,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_send);
    reveal(finish_route);
}

pub proof fn finish_routes_match_component(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_match_component(before).is_some() ==> finish_route_of(before) == 12,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_match_component);
    reveal(finish_route);
}

pub proof fn finish_routes_absent_specialized(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_absent_specialized(before).is_some() ==> finish_route_of(before) == 13,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_absent_specialized);
    reveal(finish_route);
}

pub proof fn finish_routes_absent_connective(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_absent_connective(before).is_some() ==> finish_route_of(before) == 14,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_absent_connective);
    reveal(finish_route);
}

pub proof fn finish_routes_receive_bind(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_receive_bind(before).is_some() ==> finish_route_of(before) == 15,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_receive_bind);
    reveal(finish_route);
}

pub proof fn finish_routes_match_case(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_match_case(before).is_some() ==> finish_route_of(before) == 16,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_match_case);
    reveal(finish_route);
}

pub proof fn finish_routes_bool_oneof(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_bool_oneof(before).is_some() ==> finish_route_of(before) == 17,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_bool_oneof);
    reveal(finish_route);
}

pub proof fn finish_routes_expr_atom(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_expr_atom(before).is_some() ==> finish_route_of(before) == 18,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_expr_atom);
    reveal(finish_route);
}

pub proof fn finish_routes_expr_numeric(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_expr_numeric(before).is_some() ==> finish_route_of(before) == 19,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_expr_numeric);
    reveal(finish_route);
}

pub proof fn finish_routes_expr_big_rational(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_expr_big_rational(before).is_some() ==> finish_route_of(before) == 20,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_expr_big_rational);
    reveal(finish_route);
}

pub proof fn finish_routes_expr_fixed_point(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_expr_fixed_point(before).is_some() ==> finish_route_of(before) == 21,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_expr_fixed_point);
    reveal(finish_route);
}

pub proof fn finish_routes_sys_auth_token(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_sys_auth_token(before).is_some() ==> finish_route_of(before) == 22,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_sys_auth_token);
    reveal(finish_route);
    let top = before.admission.frames.last();
    if finish_candidate_sys_auth_token(before).is_some() {
        assert(top.returns.len() == 1);
        assert(top.seen.contains(top.returns[0].slot as int));
    }
}

pub proof fn finish_routes_new_empty_collections(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_new_empty_collections(before).is_some()
        ==> finish_route_of(before) == 23,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_new_empty_collections);
    reveal(finish_route);
}

pub proof fn finish_routes_new_one_injection(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_new_one_injection(before).is_some() ==> finish_route_of(before) == 24,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_new_one_injection);
    reveal(finish_route);
}

pub proof fn finish_routes_new_one_absent_injection(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_new_one_absent_injection(before).is_some()
        ==> finish_route_of(before) == 25,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_new_one_absent_injection);
    reveal(finish_route);
}

pub proof fn finish_routes_new_one_uri(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_new_one_uri(before).is_some() ==> finish_route_of(before) == 26,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_new_one_uri);
    reveal(finish_route);
}

pub proof fn finish_routes_new_collections(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_new_collections(before).is_some() ==> finish_route_of(before) == 27,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_new_collections);
    reveal(finish_route);
}

pub proof fn finish_routes_expr_payload_forward(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_expr_payload_forward(before).is_some()
        ==> finish_route_of(before) == 28,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_expr_payload_forward);
    reveal(finish_route);
}

pub proof fn finish_routes_expr_structural(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_expr_structural(before).is_some() ==> finish_route_of(before) == 29,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_expr_structural);
    reveal(finish_route);
}

pub proof fn finish_routes_evar(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_evar(before).is_some() ==> finish_route_of(before) == 30,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_evar);
    reveal(finish_route);
    let top = before.admission.frames.last();
    if top.family == admission::AdmissionFamily::EVarFrame
        && !admission::admission_returned_var(top, 0)
    {
        bridge::bridge_evar_missing_var_step_none(before);
    }
}

pub proof fn finish_routes_ordered_sequence(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_ordered_sequence(before).is_some() ==> finish_route_of(before) == 31,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_ordered_sequence);
    reveal(finish_route);
}

pub proof fn finish_routes_emethod(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_emethod(before).is_some() ==> finish_route_of(before) == 32,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_emethod);
    reveal(finish_route);
}

pub proof fn finish_routes_emap_finish(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_emap_finish(before).is_some() ==> finish_route_of(before) == 33,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_emap_finish);
    reveal(finish_route);
}

pub proof fn finish_routes_connective_not(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_connective_not(before).is_some() ==> finish_route_of(before) == 34,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_connective_not);
    reveal(finish_route);
    let top = before.admission.frames.last();
    if finish_candidate_connective_not(before).is_some() {
        assert(top.returns.len() == 1);
        assert(top.seen.contains(top.returns[0].slot as int));
    }
}

pub proof fn finish_routes_connective_varref(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_connective_varref(before).is_some() ==> finish_route_of(before) == 35,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_connective_varref);
    reveal(finish_route);
}

pub proof fn finish_routes_connective_collection(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_connective_collection(before).is_some()
        ==> finish_route_of(before) == 36,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_connective_collection);
    reveal(finish_route);
}

pub proof fn finish_routes_unforgeable_bytes(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_unforgeable_bytes(before).is_some() ==> finish_route_of(before) == 37,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_unforgeable_bytes);
    reveal(finish_route);
}

pub proof fn finish_routes_receive_present_condition(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_receive_present_condition(before).is_some()
        ==> finish_route_of(before) == 38,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_receive_present_condition);
    reveal(finish_route);
    let top = before.admission.frames.last();
    if top.family == admission::AdmissionFamily::Receive
        && admission::admission_family_complete(top)
        && !top.seen.contains(7)
    {
        bridge::bridge_receive_absent_condition_excludes_present(before);
    }
}

pub proof fn finish_routes_receive_absent_condition(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_receive_absent_condition(before).is_some()
        ==> finish_route_of(before) == 39,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_receive_absent_condition);
    reveal(finish_route);
}

pub proof fn finish_routes_nonsemantic(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_nonsemantic(before).is_some() ==> finish_route_of(before) == 40,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_nonsemantic);
    reveal(finish_route);
}

pub proof fn finish_routes_var(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_var(before).is_some() ==> finish_route_of(before) == 41,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_var);
    reveal(finish_route);
}

pub proof fn finish_routes_epathmap_owner(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_epathmap_owner(before).is_some() ==> finish_route_of(before) == 42,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_epathmap_owner);
    reveal(finish_route);
}

pub proof fn finish_routes_ezipper(before: bridge::ScoreAdmissionBridge)
    requires finish_top_wf(before),
    ensures finish_candidate_ezipper(before).is_some() ==> finish_route_of(before) == 43,
{
    reveal(finish_top_wf);
    reveal(finish_candidate_ezipper);
    reveal(finish_route);
}

/// Group K's body is `finish_group_K_adds`.
pub proof fn finish_group_1_body(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    ensures bridge::bridge_score_finish_selection_group_1(before, selection)
        == finish_group_1_adds(before, selection),
{
    reveal(bridge::bridge_score_finish_selection_group_1);
    reveal(finish_candidate_raw_par);
    reveal(finish_candidate_cost_atom_absent);
    reveal(finish_candidate_cost_atom_bytes);
    reveal(finish_candidate_cost_bound);
    reveal(finish_candidate_cost_atom_bound_forward);
    reveal(finish_candidate_cost_signature);
    reveal(finish_candidate_cost_region);
    reveal(finish_candidate_cost_token_stack);
}

pub proof fn finish_group_2_body(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    ensures bridge::bridge_score_finish_selection_group_2(before, selection)
        == finish_group_2_adds(before, selection),
{
    reveal(bridge::bridge_score_finish_selection_group_2);
    reveal(finish_candidate_bundle);
    reveal(finish_candidate_if);
    reveal(finish_candidate_send);
    reveal(finish_candidate_match_component);
    reveal(finish_candidate_absent_specialized);
    reveal(finish_candidate_absent_connective);
    reveal(finish_candidate_receive_bind);
    reveal(finish_candidate_match_case);
}

pub proof fn finish_group_3_body(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    ensures bridge::bridge_score_finish_selection_group_3(before, selection)
        == finish_group_3_adds(before, selection),
{
    reveal(bridge::bridge_score_finish_selection_group_3);
    reveal(finish_candidate_bool_oneof);
    reveal(finish_candidate_expr_atom);
    reveal(finish_candidate_expr_numeric);
    reveal(finish_candidate_expr_big_rational);
    reveal(finish_candidate_expr_fixed_point);
    reveal(finish_candidate_sys_auth_token);
    reveal(finish_candidate_new_empty_collections);
    reveal(finish_candidate_new_one_injection);
}

pub proof fn finish_group_4_body(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    ensures bridge::bridge_score_finish_selection_group_4(before, selection)
        == finish_group_4_adds(before, selection),
{
    reveal(bridge::bridge_score_finish_selection_group_4);
    reveal(finish_candidate_new_one_absent_injection);
    reveal(finish_candidate_new_one_uri);
    reveal(finish_candidate_new_collections);
    reveal(finish_candidate_expr_payload_forward);
    reveal(finish_candidate_expr_structural);
    reveal(finish_candidate_evar);
    reveal(finish_candidate_ordered_sequence);
    reveal(finish_candidate_emethod);
    reveal(finish_candidate_emap_finish);
}

pub proof fn finish_group_5_body(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    ensures bridge::bridge_score_finish_selection_group_5(before, selection)
        == finish_group_5_adds(before, selection),
{
    reveal(bridge::bridge_score_finish_selection_group_5);
    reveal(finish_candidate_connective_not);
    reveal(finish_candidate_connective_varref);
    reveal(finish_candidate_connective_collection);
    reveal(finish_candidate_unforgeable_bytes);
    reveal(finish_candidate_receive_present_condition);
    reveal(finish_candidate_receive_absent_condition);
    reveal(finish_candidate_nonsemantic);
}

pub proof fn finish_group_6_body(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    ensures bridge::bridge_score_finish_selection_group_6(before, selection)
        == finish_group_6_adds(before, selection),
{
    reveal(bridge::bridge_score_finish_selection_group_6);
    reveal(finish_candidate_var);
    reveal(finish_candidate_epathmap_owner);
    reveal(finish_candidate_ezipper);
}

/// Fold: the opaque group adds exactly its routed candidate.
pub proof fn finish_group_1_folds(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires finish_top_wf(before),
    ensures bridge::bridge_score_finish_selection_group_1(before, selection)
        == bridge::bridge_add_score_finish_candidate(selection, finish_routed_group_1(before)),
{
    finish_group_1_body(before, selection);
    finish_routes_raw_par(before);
    finish_routes_cost_atom_absent(before);
    finish_routes_cost_atom_bytes(before);
    finish_routes_cost_bound(before);
    finish_routes_cost_atom_bound_forward(before);
    finish_routes_cost_signature(before);
    finish_routes_cost_region(before);
    finish_routes_cost_token_stack(before);
}

pub proof fn finish_group_2_folds(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires finish_top_wf(before),
    ensures bridge::bridge_score_finish_selection_group_2(before, selection)
        == bridge::bridge_add_score_finish_candidate(selection, finish_routed_group_2(before)),
{
    finish_group_2_body(before, selection);
    finish_routes_bundle(before);
    finish_routes_if(before);
    finish_routes_send(before);
    finish_routes_match_component(before);
    finish_routes_absent_specialized(before);
    finish_routes_absent_connective(before);
    finish_routes_receive_bind(before);
    finish_routes_match_case(before);
}

pub proof fn finish_group_3_folds(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires finish_top_wf(before),
    ensures bridge::bridge_score_finish_selection_group_3(before, selection)
        == bridge::bridge_add_score_finish_candidate(selection, finish_routed_group_3(before)),
{
    finish_group_3_body(before, selection);
    finish_routes_bool_oneof(before);
    finish_routes_expr_atom(before);
    finish_routes_expr_numeric(before);
    finish_routes_expr_big_rational(before);
    finish_routes_expr_fixed_point(before);
    finish_routes_sys_auth_token(before);
    finish_routes_new_empty_collections(before);
    finish_routes_new_one_injection(before);
}

pub proof fn finish_group_4_folds(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires finish_top_wf(before),
    ensures bridge::bridge_score_finish_selection_group_4(before, selection)
        == bridge::bridge_add_score_finish_candidate(selection, finish_routed_group_4(before)),
{
    finish_group_4_body(before, selection);
    finish_routes_new_one_absent_injection(before);
    finish_routes_new_one_uri(before);
    finish_routes_new_collections(before);
    finish_routes_expr_payload_forward(before);
    finish_routes_expr_structural(before);
    finish_routes_evar(before);
    finish_routes_ordered_sequence(before);
    finish_routes_emethod(before);
    finish_routes_emap_finish(before);
}

pub proof fn finish_group_5_folds(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires finish_top_wf(before),
    ensures bridge::bridge_score_finish_selection_group_5(before, selection)
        == bridge::bridge_add_score_finish_candidate(selection, finish_routed_group_5(before)),
{
    finish_group_5_body(before, selection);
    finish_routes_connective_not(before);
    finish_routes_connective_varref(before);
    finish_routes_connective_collection(before);
    finish_routes_unforgeable_bytes(before);
    finish_routes_receive_present_condition(before);
    finish_routes_receive_absent_condition(before);
    finish_routes_nonsemantic(before);
}

pub proof fn finish_group_6_folds(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires finish_top_wf(before),
    ensures bridge::bridge_score_finish_selection_group_6(before, selection)
        == bridge::bridge_add_score_finish_candidate(selection, finish_routed_group_6(before)),
{
    finish_group_6_body(before, selection);
    finish_routes_var(before);
    finish_routes_epathmap_owner(before);
    finish_routes_ezipper(before);
}

/// Sources: a group's source disjunct lies in its class and is its routed candidate. Class
/// membership needs the nonempty frame stack stated by `finish_top_wf`.
pub proof fn finish_group_1_sources(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires finish_top_wf(before), finish_group_1_source(before, after),
    ensures finish_class_1(before), finish_routed_group_1(before) == Some(after),
{
    reveal(finish_top_wf);
    finish_routes_raw_par(before);
    finish_routes_cost_atom_absent(before);
    finish_routes_cost_atom_bytes(before);
    finish_routes_cost_bound(before);
    finish_routes_cost_atom_bound_forward(before);
    finish_routes_cost_signature(before);
    finish_routes_cost_region(before);
    finish_routes_cost_token_stack(before);
}

pub proof fn finish_group_2_sources(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires finish_top_wf(before), finish_group_2_source(before, after),
    ensures finish_class_2(before), finish_routed_group_2(before) == Some(after),
{
    reveal(finish_top_wf);
    finish_routes_bundle(before);
    finish_routes_if(before);
    finish_routes_send(before);
    finish_routes_match_component(before);
    finish_routes_absent_specialized(before);
    finish_routes_absent_connective(before);
    finish_routes_receive_bind(before);
    finish_routes_match_case(before);
}

pub proof fn finish_group_3_sources(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires finish_top_wf(before), finish_group_3_source(before, after),
    ensures finish_class_3(before), finish_routed_group_3(before) == Some(after),
{
    reveal(finish_top_wf);
    finish_routes_bool_oneof(before);
    finish_routes_expr_atom(before);
    finish_routes_expr_numeric(before);
    finish_routes_expr_big_rational(before);
    finish_routes_expr_fixed_point(before);
    finish_routes_sys_auth_token(before);
    finish_routes_new_empty_collections(before);
    finish_routes_new_one_injection(before);
}

pub proof fn finish_group_4_sources(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires finish_top_wf(before), finish_group_4_source(before, after),
    ensures finish_class_4(before), finish_routed_group_4(before) == Some(after),
{
    reveal(finish_top_wf);
    finish_routes_new_one_absent_injection(before);
    finish_routes_new_one_uri(before);
    finish_routes_new_collections(before);
    finish_routes_expr_payload_forward(before);
    finish_routes_expr_structural(before);
    finish_routes_evar(before);
    finish_routes_ordered_sequence(before);
    finish_routes_emethod(before);
    finish_routes_emap_finish(before);
}

pub proof fn finish_group_5_sources(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires finish_top_wf(before), finish_group_5_source(before, after),
    ensures finish_class_5(before), finish_routed_group_5(before) == Some(after),
{
    reveal(finish_top_wf);
    finish_routes_connective_not(before);
    finish_routes_connective_varref(before);
    finish_routes_connective_collection(before);
    finish_routes_unforgeable_bytes(before);
    finish_routes_receive_present_condition(before);
    finish_routes_receive_absent_condition(before);
    finish_routes_nonsemantic(before);
}

pub proof fn finish_group_6_sources(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires finish_top_wf(before), finish_group_6_source(before, after),
    ensures finish_class_6(before), finish_routed_group_6(before) == Some(after),
{
    reveal(finish_top_wf);
    finish_routes_var(before);
    finish_routes_epathmap_owner(before);
    finish_routes_ezipper(before);
}

/// The 43 disjuncts of `bridge_score_source_step`, regrouped by fold group.
pub proof fn finish_source_step_by_group(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    ensures bridge::bridge_score_source_step(before, after) == (
        finish_group_1_source(before, after) || finish_group_2_source(before, after)
            || finish_group_3_source(before, after) || finish_group_4_source(before, after)
            || finish_group_5_source(before, after) || finish_group_6_source(before, after)
    ),
{
    reveal(bridge::bridge_score_source_step_group_6);
    reveal(finish_candidate_raw_par);
    reveal(finish_candidate_cost_atom_absent);
    reveal(finish_candidate_cost_atom_bytes);
    reveal(finish_candidate_cost_bound);
    reveal(finish_candidate_cost_atom_bound_forward);
    reveal(finish_candidate_cost_signature);
    reveal(finish_candidate_cost_region);
    reveal(finish_candidate_cost_token_stack);
    reveal(finish_candidate_bundle);
    reveal(finish_candidate_if);
    reveal(finish_candidate_send);
    reveal(finish_candidate_match_component);
    reveal(finish_candidate_absent_specialized);
    reveal(finish_candidate_absent_connective);
    reveal(finish_candidate_receive_bind);
    reveal(finish_candidate_match_case);
    reveal(finish_candidate_bool_oneof);
    reveal(finish_candidate_expr_atom);
    reveal(finish_candidate_expr_numeric);
    reveal(finish_candidate_expr_big_rational);
    reveal(finish_candidate_expr_fixed_point);
    reveal(finish_candidate_sys_auth_token);
    reveal(finish_candidate_new_empty_collections);
    reveal(finish_candidate_new_one_injection);
    reveal(finish_candidate_new_one_absent_injection);
    reveal(finish_candidate_new_one_uri);
    reveal(finish_candidate_new_collections);
    reveal(finish_candidate_expr_payload_forward);
    reveal(finish_candidate_expr_structural);
    reveal(finish_candidate_evar);
    reveal(finish_candidate_ordered_sequence);
    reveal(finish_candidate_emethod);
    reveal(finish_candidate_emap_finish);
    reveal(finish_candidate_connective_not);
    reveal(finish_candidate_connective_varref);
    reveal(finish_candidate_connective_collection);
    reveal(finish_candidate_unforgeable_bytes);
    reveal(finish_candidate_receive_present_condition);
    reveal(finish_candidate_receive_absent_condition);
    reveal(finish_candidate_nonsemantic);
    reveal(finish_candidate_var);
    reveal(finish_candidate_epathmap_owner);
    reveal(finish_candidate_ezipper);
}

/// Every successful generic finish is the routed candidate of the frame's class.
pub proof fn finish_source_step_routes(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires finish_top_wf(before), bridge::bridge_score_source_step(before, after),
    ensures
        finish_class_1(before) || finish_class_2(before) || finish_class_3(before)
            || finish_class_4(before) || finish_class_5(before) || finish_class_6(before),
        finish_routed(before) == Some(after),
{
    finish_source_step_by_group(before, after);
    if finish_group_1_source(before, after) {
        finish_group_1_sources(before, after);
    } else if finish_group_2_source(before, after) {
        finish_group_2_sources(before, after);
    } else if finish_group_3_source(before, after) {
        finish_group_3_sources(before, after);
    } else if finish_group_4_source(before, after) {
        finish_group_4_sources(before, after);
    } else if finish_group_5_source(before, after) {
        finish_group_5_sources(before, after);
    } else {
        finish_group_6_sources(before, after);
    }
}

/// Coverage of group K: on a finishing well-formed EOF prestate of class K, the group-K source
/// disjunct holds and group K adds exactly the finish to every selection.
pub proof fn finish_group_1_covers(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires
        finish_eof_prestate(before),
        finish_class_1(before),
        bridge::bridge_score_source_step(before, after),
    ensures
        finish_routed_group_1(before) == Some(after),
        finish_group_1_source(before, after),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_1(before, selection)
                == bridge::bridge_add_score_finish_candidate(selection, Some(after)),
{
    finish_eof_prestate_top_wf(before);
    finish_source_step_routes(before, after);
    finish_source_step_by_group(before, after);
    if finish_group_2_source(before, after) {
        finish_group_2_sources(before, after);
    }
    if finish_group_3_source(before, after) {
        finish_group_3_sources(before, after);
    }
    if finish_group_4_source(before, after) {
        finish_group_4_sources(before, after);
    }
    if finish_group_5_source(before, after) {
        finish_group_5_sources(before, after);
    }
    if finish_group_6_source(before, after) {
        finish_group_6_sources(before, after);
    }
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_1(before, selection)
            == bridge::bridge_add_score_finish_candidate(selection, Some(after)) by {
        finish_group_1_folds(before, selection);
    }
}

pub proof fn finish_group_2_covers(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires
        finish_eof_prestate(before),
        finish_class_2(before),
        bridge::bridge_score_source_step(before, after),
    ensures
        finish_routed_group_2(before) == Some(after),
        finish_group_2_source(before, after),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_2(before, selection)
                == bridge::bridge_add_score_finish_candidate(selection, Some(after)),
{
    finish_eof_prestate_top_wf(before);
    finish_source_step_routes(before, after);
    finish_source_step_by_group(before, after);
    if finish_group_1_source(before, after) {
        finish_group_1_sources(before, after);
    }
    if finish_group_3_source(before, after) {
        finish_group_3_sources(before, after);
    }
    if finish_group_4_source(before, after) {
        finish_group_4_sources(before, after);
    }
    if finish_group_5_source(before, after) {
        finish_group_5_sources(before, after);
    }
    if finish_group_6_source(before, after) {
        finish_group_6_sources(before, after);
    }
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_2(before, selection)
            == bridge::bridge_add_score_finish_candidate(selection, Some(after)) by {
        finish_group_2_folds(before, selection);
    }
}

pub proof fn finish_group_3_covers(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires
        finish_eof_prestate(before),
        finish_class_3(before),
        bridge::bridge_score_source_step(before, after),
    ensures
        finish_routed_group_3(before) == Some(after),
        finish_group_3_source(before, after),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_3(before, selection)
                == bridge::bridge_add_score_finish_candidate(selection, Some(after)),
{
    finish_eof_prestate_top_wf(before);
    finish_source_step_routes(before, after);
    finish_source_step_by_group(before, after);
    if finish_group_1_source(before, after) {
        finish_group_1_sources(before, after);
    }
    if finish_group_2_source(before, after) {
        finish_group_2_sources(before, after);
    }
    if finish_group_4_source(before, after) {
        finish_group_4_sources(before, after);
    }
    if finish_group_5_source(before, after) {
        finish_group_5_sources(before, after);
    }
    if finish_group_6_source(before, after) {
        finish_group_6_sources(before, after);
    }
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_3(before, selection)
            == bridge::bridge_add_score_finish_candidate(selection, Some(after)) by {
        finish_group_3_folds(before, selection);
    }
}

pub proof fn finish_group_4_covers(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires
        finish_eof_prestate(before),
        finish_class_4(before),
        bridge::bridge_score_source_step(before, after),
    ensures
        finish_routed_group_4(before) == Some(after),
        finish_group_4_source(before, after),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_4(before, selection)
                == bridge::bridge_add_score_finish_candidate(selection, Some(after)),
{
    finish_eof_prestate_top_wf(before);
    finish_source_step_routes(before, after);
    finish_source_step_by_group(before, after);
    if finish_group_1_source(before, after) {
        finish_group_1_sources(before, after);
    }
    if finish_group_2_source(before, after) {
        finish_group_2_sources(before, after);
    }
    if finish_group_3_source(before, after) {
        finish_group_3_sources(before, after);
    }
    if finish_group_5_source(before, after) {
        finish_group_5_sources(before, after);
    }
    if finish_group_6_source(before, after) {
        finish_group_6_sources(before, after);
    }
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_4(before, selection)
            == bridge::bridge_add_score_finish_candidate(selection, Some(after)) by {
        finish_group_4_folds(before, selection);
    }
}

pub proof fn finish_group_5_covers(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires
        finish_eof_prestate(before),
        finish_class_5(before),
        bridge::bridge_score_source_step(before, after),
    ensures
        finish_routed_group_5(before) == Some(after),
        finish_group_5_source(before, after),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_5(before, selection)
                == bridge::bridge_add_score_finish_candidate(selection, Some(after)),
{
    finish_eof_prestate_top_wf(before);
    finish_source_step_routes(before, after);
    finish_source_step_by_group(before, after);
    if finish_group_1_source(before, after) {
        finish_group_1_sources(before, after);
    }
    if finish_group_2_source(before, after) {
        finish_group_2_sources(before, after);
    }
    if finish_group_3_source(before, after) {
        finish_group_3_sources(before, after);
    }
    if finish_group_4_source(before, after) {
        finish_group_4_sources(before, after);
    }
    if finish_group_6_source(before, after) {
        finish_group_6_sources(before, after);
    }
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_5(before, selection)
            == bridge::bridge_add_score_finish_candidate(selection, Some(after)) by {
        finish_group_5_folds(before, selection);
    }
}

pub proof fn finish_group_6_covers(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires
        finish_eof_prestate(before),
        finish_class_6(before),
        bridge::bridge_score_source_step(before, after),
    ensures
        finish_routed_group_6(before) == Some(after),
        finish_group_6_source(before, after),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_6(before, selection)
                == bridge::bridge_add_score_finish_candidate(selection, Some(after)),
{
    finish_eof_prestate_top_wf(before);
    finish_source_step_routes(before, after);
    finish_source_step_by_group(before, after);
    if finish_group_1_source(before, after) {
        finish_group_1_sources(before, after);
    }
    if finish_group_2_source(before, after) {
        finish_group_2_sources(before, after);
    }
    if finish_group_3_source(before, after) {
        finish_group_3_sources(before, after);
    }
    if finish_group_4_source(before, after) {
        finish_group_4_sources(before, after);
    }
    if finish_group_5_source(before, after) {
        finish_group_5_sources(before, after);
    }
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_6(before, selection)
            == bridge::bridge_add_score_finish_candidate(selection, Some(after)) by {
        finish_group_6_folds(before, selection);
    }
}

/// Disjointness of group K: its candidates fire only at their own route index, it adds at most
/// its routed candidate, and it leaves every selection unchanged outside class K.
pub proof fn finish_group_1_disjoint(before: bridge::ScoreAdmissionBridge)
    requires finish_eof_prestate(before),
    ensures
        finish_candidate_raw_par(before).is_some() ==> finish_route_of(before) == 1,
        finish_candidate_cost_atom_absent(before).is_some() ==> finish_route_of(before) == 2,
        finish_candidate_cost_atom_bytes(before).is_some() ==> finish_route_of(before) == 3,
        finish_candidate_cost_bound(before).is_some() ==> finish_route_of(before) == 4,
        finish_candidate_cost_atom_bound_forward(before).is_some() ==> finish_route_of(before) == 5,
        finish_candidate_cost_signature(before).is_some() ==> finish_route_of(before) == 6,
        finish_candidate_cost_region(before).is_some() ==> finish_route_of(before) == 7,
        finish_candidate_cost_token_stack(before).is_some() ==> finish_route_of(before) == 8,
        !finish_class_1(before) ==> finish_routed_group_1(before).is_none(),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_1(before, selection)
                == bridge::bridge_add_score_finish_candidate(
                    selection, finish_routed_group_1(before),
                ),
        !finish_class_1(before) ==> forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_1(before, selection)
                == selection,
{
    finish_eof_prestate_top_wf(before);
    finish_routes_raw_par(before);
    finish_routes_cost_atom_absent(before);
    finish_routes_cost_atom_bytes(before);
    finish_routes_cost_bound(before);
    finish_routes_cost_atom_bound_forward(before);
    finish_routes_cost_signature(before);
    finish_routes_cost_region(before);
    finish_routes_cost_token_stack(before);
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_1(before, selection)
            == bridge::bridge_add_score_finish_candidate(
                selection, finish_routed_group_1(before),
            ) by {
        finish_group_1_folds(before, selection);
    }
}

pub proof fn finish_group_2_disjoint(before: bridge::ScoreAdmissionBridge)
    requires finish_eof_prestate(before),
    ensures
        finish_candidate_bundle(before).is_some() ==> finish_route_of(before) == 9,
        finish_candidate_if(before).is_some() ==> finish_route_of(before) == 10,
        finish_candidate_send(before).is_some() ==> finish_route_of(before) == 11,
        finish_candidate_match_component(before).is_some() ==> finish_route_of(before) == 12,
        finish_candidate_absent_specialized(before).is_some() ==> finish_route_of(before) == 13,
        finish_candidate_absent_connective(before).is_some() ==> finish_route_of(before) == 14,
        finish_candidate_receive_bind(before).is_some() ==> finish_route_of(before) == 15,
        finish_candidate_match_case(before).is_some() ==> finish_route_of(before) == 16,
        !finish_class_2(before) ==> finish_routed_group_2(before).is_none(),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_2(before, selection)
                == bridge::bridge_add_score_finish_candidate(
                    selection, finish_routed_group_2(before),
                ),
        !finish_class_2(before) ==> forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_2(before, selection)
                == selection,
{
    finish_eof_prestate_top_wf(before);
    finish_routes_bundle(before);
    finish_routes_if(before);
    finish_routes_send(before);
    finish_routes_match_component(before);
    finish_routes_absent_specialized(before);
    finish_routes_absent_connective(before);
    finish_routes_receive_bind(before);
    finish_routes_match_case(before);
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_2(before, selection)
            == bridge::bridge_add_score_finish_candidate(
                selection, finish_routed_group_2(before),
            ) by {
        finish_group_2_folds(before, selection);
    }
}

pub proof fn finish_group_3_disjoint(before: bridge::ScoreAdmissionBridge)
    requires finish_eof_prestate(before),
    ensures
        finish_candidate_bool_oneof(before).is_some() ==> finish_route_of(before) == 17,
        finish_candidate_expr_atom(before).is_some() ==> finish_route_of(before) == 18,
        finish_candidate_expr_numeric(before).is_some() ==> finish_route_of(before) == 19,
        finish_candidate_expr_big_rational(before).is_some() ==> finish_route_of(before) == 20,
        finish_candidate_expr_fixed_point(before).is_some() ==> finish_route_of(before) == 21,
        finish_candidate_sys_auth_token(before).is_some() ==> finish_route_of(before) == 22,
        finish_candidate_new_empty_collections(before).is_some() ==> finish_route_of(before) == 23,
        finish_candidate_new_one_injection(before).is_some() ==> finish_route_of(before) == 24,
        !finish_class_3(before) ==> finish_routed_group_3(before).is_none(),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_3(before, selection)
                == bridge::bridge_add_score_finish_candidate(
                    selection, finish_routed_group_3(before),
                ),
        !finish_class_3(before) ==> forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_3(before, selection)
                == selection,
{
    finish_eof_prestate_top_wf(before);
    finish_routes_bool_oneof(before);
    finish_routes_expr_atom(before);
    finish_routes_expr_numeric(before);
    finish_routes_expr_big_rational(before);
    finish_routes_expr_fixed_point(before);
    finish_routes_sys_auth_token(before);
    finish_routes_new_empty_collections(before);
    finish_routes_new_one_injection(before);
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_3(before, selection)
            == bridge::bridge_add_score_finish_candidate(
                selection, finish_routed_group_3(before),
            ) by {
        finish_group_3_folds(before, selection);
    }
}

pub proof fn finish_group_4_disjoint(before: bridge::ScoreAdmissionBridge)
    requires finish_eof_prestate(before),
    ensures
        finish_candidate_new_one_absent_injection(before).is_some()
            ==> finish_route_of(before) == 25,
        finish_candidate_new_one_uri(before).is_some() ==> finish_route_of(before) == 26,
        finish_candidate_new_collections(before).is_some() ==> finish_route_of(before) == 27,
        finish_candidate_expr_payload_forward(before).is_some() ==> finish_route_of(before) == 28,
        finish_candidate_expr_structural(before).is_some() ==> finish_route_of(before) == 29,
        finish_candidate_evar(before).is_some() ==> finish_route_of(before) == 30,
        finish_candidate_ordered_sequence(before).is_some() ==> finish_route_of(before) == 31,
        finish_candidate_emethod(before).is_some() ==> finish_route_of(before) == 32,
        finish_candidate_emap_finish(before).is_some() ==> finish_route_of(before) == 33,
        !finish_class_4(before) ==> finish_routed_group_4(before).is_none(),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_4(before, selection)
                == bridge::bridge_add_score_finish_candidate(
                    selection, finish_routed_group_4(before),
                ),
        !finish_class_4(before) ==> forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_4(before, selection)
                == selection,
{
    finish_eof_prestate_top_wf(before);
    finish_routes_new_one_absent_injection(before);
    finish_routes_new_one_uri(before);
    finish_routes_new_collections(before);
    finish_routes_expr_payload_forward(before);
    finish_routes_expr_structural(before);
    finish_routes_evar(before);
    finish_routes_ordered_sequence(before);
    finish_routes_emethod(before);
    finish_routes_emap_finish(before);
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_4(before, selection)
            == bridge::bridge_add_score_finish_candidate(
                selection, finish_routed_group_4(before),
            ) by {
        finish_group_4_folds(before, selection);
    }
}

pub proof fn finish_group_5_disjoint(before: bridge::ScoreAdmissionBridge)
    requires finish_eof_prestate(before),
    ensures
        finish_candidate_connective_not(before).is_some() ==> finish_route_of(before) == 34,
        finish_candidate_connective_varref(before).is_some() ==> finish_route_of(before) == 35,
        finish_candidate_connective_collection(before).is_some() ==> finish_route_of(before) == 36,
        finish_candidate_unforgeable_bytes(before).is_some() ==> finish_route_of(before) == 37,
        finish_candidate_receive_present_condition(before).is_some()
            ==> finish_route_of(before) == 38,
        finish_candidate_receive_absent_condition(before).is_some()
            ==> finish_route_of(before) == 39,
        finish_candidate_nonsemantic(before).is_some() ==> finish_route_of(before) == 40,
        !finish_class_5(before) ==> finish_routed_group_5(before).is_none(),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_5(before, selection)
                == bridge::bridge_add_score_finish_candidate(
                    selection, finish_routed_group_5(before),
                ),
        !finish_class_5(before) ==> forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_5(before, selection)
                == selection,
{
    finish_eof_prestate_top_wf(before);
    finish_routes_connective_not(before);
    finish_routes_connective_varref(before);
    finish_routes_connective_collection(before);
    finish_routes_unforgeable_bytes(before);
    finish_routes_receive_present_condition(before);
    finish_routes_receive_absent_condition(before);
    finish_routes_nonsemantic(before);
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_5(before, selection)
            == bridge::bridge_add_score_finish_candidate(
                selection, finish_routed_group_5(before),
            ) by {
        finish_group_5_folds(before, selection);
    }
}

pub proof fn finish_group_6_disjoint(before: bridge::ScoreAdmissionBridge)
    requires finish_eof_prestate(before),
    ensures
        finish_candidate_var(before).is_some() ==> finish_route_of(before) == 41,
        finish_candidate_epathmap_owner(before).is_some() ==> finish_route_of(before) == 42,
        finish_candidate_ezipper(before).is_some() ==> finish_route_of(before) == 43,
        !finish_class_6(before) ==> finish_routed_group_6(before).is_none(),
        forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_6(before, selection)
                == bridge::bridge_add_score_finish_candidate(
                    selection, finish_routed_group_6(before),
                ),
        !finish_class_6(before) ==> forall|selection: bridge::BridgeScoreFinishSelection|
            #[trigger] bridge::bridge_score_finish_selection_group_6(before, selection)
                == selection,
{
    finish_eof_prestate_top_wf(before);
    finish_routes_var(before);
    finish_routes_epathmap_owner(before);
    finish_routes_ezipper(before);
    assert forall|selection: bridge::BridgeScoreFinishSelection|
        #[trigger] bridge::bridge_score_finish_selection_group_6(before, selection)
            == bridge::bridge_add_score_finish_candidate(
                selection, finish_routed_group_6(before),
            ) by {
        finish_group_6_folds(before, selection);
    }
}

/// The six classes partition the finishing well-formed EOF prestates.
pub proof fn finish_classes_partition(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    requires finish_eof_prestate(before), bridge::bridge_score_source_step(before, after),
    ensures
        finish_class_1(before) || finish_class_2(before) || finish_class_3(before)
            || finish_class_4(before) || finish_class_5(before) || finish_class_6(before),
        !(finish_class_1(before) && finish_class_2(before)),
        !(finish_class_1(before) && finish_class_3(before)),
        !(finish_class_1(before) && finish_class_4(before)),
        !(finish_class_1(before) && finish_class_5(before)),
        !(finish_class_1(before) && finish_class_6(before)),
        !(finish_class_2(before) && finish_class_3(before)),
        !(finish_class_2(before) && finish_class_4(before)),
        !(finish_class_2(before) && finish_class_5(before)),
        !(finish_class_2(before) && finish_class_6(before)),
        !(finish_class_3(before) && finish_class_4(before)),
        !(finish_class_3(before) && finish_class_5(before)),
        !(finish_class_3(before) && finish_class_6(before)),
        !(finish_class_4(before) && finish_class_5(before)),
        !(finish_class_4(before) && finish_class_6(before)),
        !(finish_class_5(before) && finish_class_6(before)),
{
    finish_eof_prestate_top_wf(before);
    finish_source_step_routes(before, after);
}

/// The score part of the runtime's modeled no-score child, `runtime_noscore_receive_bind_before`: a
/// ReceiveBindFrame without source, pushed under a Receive parent, at its EOF.
#[verifier::opaque]
pub open spec fn finish_uncovered_prestate() -> bridge::ScoreAdmissionBridge {
    runtime_source::runtime_noscore_receive_bind_before().score
}

/// The uncovered prestate is a well-formed EOF prestate whose top frame has route 0.
pub proof fn finish_uncovered_shape()
    ensures
        finish_eof_prestate(finish_uncovered_prestate()),
        finish_route_of(finish_uncovered_prestate()) == 0,
{
    reveal(finish_uncovered_prestate);
    reveal(finish_eof_prestate);
    reveal(finish_route);
    let before = finish_uncovered_prestate();
    let state = before.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 19 || program_type_id == 16)
        implies #[trigger] admission::admission_descriptor_not_pathmap(
            program_type_id, tag,
        ) by {
    }
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 19 || program_type_id == 16)
        implies #[trigger] admission::admission_no_required_descriptor_slot(
            program_type_id, slot,
        ) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
    assert forall|frame_index: int| 0 <= frame_index < state.frames.len()
        implies admission::admission_required_slots_generated(
            state.frames[frame_index],
        ) by {
        let frame = state.frames[frame_index];
        assert(frame.required == Set::<int>::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 19u16 } else { 16u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission::admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    admission::admission_fabricated_child_physical_contained(
        admission::AdmissionFamily::Receive, 0,
        admission::AdmissionFamily::ReceiveBindFrame, 1,
    );
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before));
    let top = state.frames.last();
    assert(top == child);
    assert(top.family == admission::AdmissionFamily::ReceiveBindFrame);
    assert(top.returns.len() == 0);
    assert(!admission::admission_family_complete(top));
}

/// Coverage needs the finish premise: the uncovered prestate is a well-formed EOF prestate in no
/// class, it admits no generic finish, and the selector returns `None` on it.
pub proof fn finish_coverage_counterexample()
    ensures
        finish_eof_prestate(finish_uncovered_prestate()),
        finish_route_of(finish_uncovered_prestate()) == 0,
        !finish_class_1(finish_uncovered_prestate()),
        !finish_class_2(finish_uncovered_prestate()),
        !finish_class_3(finish_uncovered_prestate()),
        !finish_class_4(finish_uncovered_prestate()),
        !finish_class_5(finish_uncovered_prestate()),
        !finish_class_6(finish_uncovered_prestate()),
        forall|after: bridge::ScoreAdmissionBridge|
            !#[trigger] bridge::bridge_score_source_step(finish_uncovered_prestate(), after),
        bridge::bridge_score_finish_selected(finish_uncovered_prestate()).is_none(),
{
    finish_uncovered_shape();
    let before = finish_uncovered_prestate();
    finish_eof_prestate_top_wf(before);
    assert forall|after: bridge::ScoreAdmissionBridge|
        !#[trigger] bridge::bridge_score_source_step(before, after) by {
        if bridge::bridge_score_source_step(before, after) {
            finish_source_step_routes(before, after);
        }
    }
    finish_group_1_disjoint(before);
    finish_group_2_disjoint(before);
    finish_group_3_disjoint(before);
    finish_group_4_disjoint(before);
    finish_group_5_disjoint(before);
    finish_group_6_disjoint(before);
    reveal(bridge::bridge_score_finish_selected);
}

/// A one-frame root at EOF of an empty region, built from the admission module's family witness
/// frame.
pub open spec fn finish_witness_root(
    family: admission::AdmissionFamily, seen: Set<int>,
    returns: Seq<admission::AdmissionReturnedSummary>, program: u16, slot_count: usize,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        program_type_id: program,
        slot_count,
        body_id: 0,
        score_handle: None,
        ..admission::admission_family_witness_frame(family, seen, returns)
    }
}

pub open spec fn finish_witness_unit_return() -> admission::AdmissionReturnedSummary {
    admission::AdmissionReturnedSummary {
        slot: 0,
        tag: 4,
        body_id: 0,
        body_bytes: Seq::empty(),
        score_handle: None,
        score_origin_body_id: None,
        kind: admission::AdmissionReturnedKind::Unit,
    }
}

/// Class 1: an empty CostTokenStack (route 8).
#[verifier::opaque]
pub open spec fn finish_witness_class_1() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_initial(finish_witness_root(
        admission::AdmissionFamily::CostTokenStack, Set::empty(), Seq::empty(), 13, 1,
    ))
}

/// Class 2: an Expr with its oneof absent (route 13).
#[verifier::opaque]
pub open spec fn finish_witness_class_2() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_initial(finish_witness_root(
        admission::AdmissionFamily::Expr, Set::empty(), Seq::empty(), 24, 1,
    ))
}

/// Class 3: a GUnforgeable carrying the system auth token arm (route 22).
#[verifier::opaque]
pub open spec fn finish_witness_class_3() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_initial(finish_witness_root(
        admission::AdmissionFamily::GUnforgeable, Set::empty().insert(0),
        seq![finish_witness_unit_return()], 60, 1,
    ))
}

/// Class 4: an empty EList message (route 31).
#[verifier::opaque]
pub open spec fn finish_witness_class_4() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_initial(finish_witness_root(
        admission::AdmissionFamily::Other, Set::empty(), Seq::empty(), 27, 4,
    ))
}

/// Class 5: an empty, hence incomplete, Send (route 40).
#[verifier::opaque]
pub open spec fn finish_witness_class_5() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_initial(finish_witness_root(
        admission::AdmissionFamily::Send, Set::empty(), Seq::empty(), 15, 6,
    ))
}

/// Class 6, Var: the bridge's EVar parent with its Var child on top at EOF (route 41).
#[verifier::opaque]
pub open spec fn finish_witness_var() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_var_child_witness_before()
}

/// Class 6, EPathMap owner: the bridge's one-frame program-31 owner holding its EPM1 snapshot
/// return (route 42).
#[verifier::opaque]
pub open spec fn finish_witness_owner() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_epathmap_witness_owner()
}

/// Class 6, EZipper: the bridge's program-32 frame after its owner child scored and completed into
/// slot 0 (route 43).
#[verifier::opaque]
pub open spec fn finish_witness_ezipper() -> bridge::ScoreAdmissionBridge {
    bridge::bridge_ezipper_witness_after_child().unwrap()
}

/// A one-frame initial bridge whose root is well formed and at EOF is a well-formed EOF prestate.
pub proof fn finish_witness_prestate(root: admission::AdmissionFrameState)
    requires
        admission::admission_frame_wf(root),
        root.cursor == root.region_end,
        root.waiting_id.is_none(),
        root.nested_started == root.nested_resolved,
        root.score_handle.is_none(),
        root.return_id.is_none(),
        forall|i: int| 0 <= i < root.returns.len()
            ==> (#[trigger] root.returns[i]).score_handle.is_none()
                && root.returns[i].score_origin_body_id.is_none(),
    ensures finish_eof_prestate(bridge::bridge_initial(root)),
{
    reveal(finish_eof_prestate);
    bridge::bridge_initial_exact(root);
    let state = admission::admission_initial_state(root);
    assert(state.frames.len() == 1);
    assert(state.frames[0] == root);
    assert(admission::admission_state_wf(state));
}

/// On such a prestate, the leaf constructor finish (constructor integer, then a one-child node,
/// then attach) succeeds.
pub proof fn finish_witness_leaf_fires(root: admission::AdmissionFrameState, constructor: i32)
    requires
        finish_eof_prestate(bridge::bridge_initial(root)),
        admission::admission_slots_complete(root),
        admission::admission_family_admissible(root),
        root.family != admission::AdmissionFamily::ReceiveBindFrame,
        root.family != admission::AdmissionFamily::MatchCaseFrame,
    ensures
        bridge::bridge_constructor_step(
            bridge::bridge_initial(root), constructor, Seq::empty(),
        ).is_some(),
{
    reveal(finish_eof_prestate);
    let before = bridge::bridge_initial(root);
    bridge::bridge_initial_exact(root);
    let constructor_node = score::ScoreNode::Integer(constructor as i64);
    assert(score::integer_transcript(constructor as i64).len() == 10);
    assert(score::checked_append(before.transcript, constructor_node).is_some());
    assert(bridge::bridge_append_unattached(before, constructor_node).is_some());
    bridge::bridge_append_unattached_exact(before, constructor_node);
    let (after_constructor, constructor_handle) =
        bridge::bridge_append_unattached(before, constructor_node).unwrap();
    let node = score::ScoreNode::Node(seq![constructor_handle]);
    assert(constructor_handle == 0);
    assert(after_constructor.transcript.entries.len() == 1);
    assert(score::arena_well_formed(after_constructor.transcript));
    assert(score::child_handles_valid(
        after_constructor.transcript.entries, seq![constructor_handle],
    ));
    assert(after_constructor.transcript.entries[0].transcript.len() == 10);
    let children = seq![constructor_handle];
    assert(children.len() == 1);
    assert(children.drop_first() == Seq::<usize>::empty());
    assert(score::children_transcript(
        after_constructor.transcript.entries, children.drop_first(),
    ) == Seq::<u8>::empty());
    assert(score::children_transcript(after_constructor.transcript.entries, children)
        == after_constructor.transcript.entries[0].transcript);
    assert(score::kind_transcript(after_constructor.transcript.entries, node).len() == 12);
    assert(score::checked_append(after_constructor.transcript, node).is_some());
    assert(bridge::bridge_append_unattached(after_constructor, node).is_some());
    bridge::bridge_append_unattached_exact(after_constructor, node);
    let (after_node, score_handle) =
        bridge::bridge_append_unattached(after_constructor, node).unwrap();
    assert(admission::admission_attach_score_step(
        after_node.admission, score_handle, root.body_id,
    ).is_some());
    assert(score::child_handles_valid(before.transcript.entries, Seq::<usize>::empty()));
    assert(seq![constructor_handle] + Seq::<usize>::empty() == seq![constructor_handle]);
}

/// The class-1 witness is a well-formed EOF prestate of class 1 on which the CostTokenStack finish
/// fires.
pub proof fn finish_witness_cost_token_stack_fires()
    ensures
        finish_eof_prestate(finish_witness_class_1()),
        finish_class_1(finish_witness_class_1()),
        finish_candidate_cost_token_stack(finish_witness_class_1()).is_some(),
{
    reveal(finish_witness_class_1);
    reveal(finish_candidate_cost_token_stack);
    reveal(finish_route);
    let root = finish_witness_root(
        admission::AdmissionFamily::CostTokenStack, Set::empty(), Seq::empty(), 13, 1,
    );
    assert forall|slot: int| 0 <= slot < root.slot_count as int
        implies root.required.contains(slot) == (exists|tag: u32|
            admission::admission_required_descriptor_slot(root.program_type_id, tag, slot)) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(13, tag, slot) by {}
    }
    assert(admission::admission_frame_wf(root));
    finish_witness_prestate(root);
    finish_witness_leaf_fires(root, 508i32);
    assert(bridge::bridge_cost_layers(root.returns, Seq::empty()) == Some(Seq::<usize>::empty()));
}

/// The class-2 witness is a well-formed EOF prestate of class 2 on which the absent-oneof finish
/// (constructor 0) fires.
pub proof fn finish_witness_absent_specialized_fires()
    ensures
        finish_eof_prestate(finish_witness_class_2()),
        finish_class_2(finish_witness_class_2()),
        finish_candidate_absent_specialized(finish_witness_class_2()).is_some(),
{
    reveal(finish_witness_class_2);
    reveal(finish_candidate_absent_specialized);
    reveal(finish_route);
    let root = finish_witness_root(
        admission::AdmissionFamily::Expr, Set::empty(), Seq::empty(), 24, 1,
    );
    assert forall|slot: int| 0 <= slot < root.slot_count as int
        implies root.required.contains(slot) == (exists|tag: u32|
            admission::admission_required_descriptor_slot(root.program_type_id, tag, slot)) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(24, tag, slot) by {}
    }
    assert(admission::admission_frame_wf(root));
    finish_witness_prestate(root);
    assert(admission::admission_family_complete(root));
    finish_witness_leaf_fires(root, 0i32);
}

/// The class-3 witness is a well-formed EOF prestate of class 3 on which the system auth token
/// finish (constructor 12) fires.
pub proof fn finish_witness_sys_auth_token_fires()
    ensures
        finish_eof_prestate(finish_witness_class_3()),
        finish_class_3(finish_witness_class_3()),
        finish_candidate_sys_auth_token(finish_witness_class_3()).is_some(),
{
    reveal(finish_witness_class_3);
    reveal(finish_candidate_sys_auth_token);
    reveal(finish_route);
    let root = finish_witness_root(
        admission::AdmissionFamily::GUnforgeable, Set::empty().insert(0),
        seq![finish_witness_unit_return()], 60, 1,
    );
    assert forall|slot: int| 0 <= slot < root.slot_count as int
        implies root.required.contains(slot) == (exists|tag: u32|
            admission::admission_required_descriptor_slot(root.program_type_id, tag, slot)) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(60, tag, slot) by {}
    }
    assert(root.returns[0] == finish_witness_unit_return());
    assert(admission::admission_frame_wf(root));
    finish_witness_prestate(root);
    finish_witness_leaf_fires(root, 12i32);
}

/// The class-4 witness is a well-formed EOF prestate of class 4 on which the ordered-sequence
/// finish fires: default remainder -1, flag 0, constructor 6, node [6, -1, 0], attach.
pub proof fn finish_witness_ordered_sequence_fires()
    ensures
        finish_eof_prestate(finish_witness_class_4()),
        finish_class_4(finish_witness_class_4()),
        finish_candidate_ordered_sequence(finish_witness_class_4()).is_some(),
{
    reveal(finish_witness_class_4);
    reveal(finish_candidate_ordered_sequence);
    reveal(finish_route);
    let root = finish_witness_root(
        admission::AdmissionFamily::Other, Set::empty(), Seq::empty(), 27, 4,
    );
    assert forall|slot: int| 0 <= slot < root.slot_count as int
        implies root.required.contains(slot) == (exists|tag: u32|
            admission::admission_required_descriptor_slot(root.program_type_id, tag, slot)) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(27, tag, slot) by {}
    }
    assert(admission::admission_frame_wf(root));
    finish_witness_prestate(root);
    let before = finish_witness_class_4();
    reveal(finish_eof_prestate);
    bridge::bridge_initial_exact(root);
    let arena = before.admission.arena;
    assert(bridge::bridge_par_handles_for_slot(root.returns, arena, 0, 1)
        == Some(Seq::<usize>::empty()));
    assert(bridge::bridge_var_handles_for_slot(root.returns, arena, 3, 5)
        == Some(Seq::<usize>::empty()));
    assert(bridge::bridge_scalar_slot(root.scalar_occurrences, 2, 4, 1u8)
        == Some(None::<u64>));
    assert(bridge::bridge_bool_slot(root, 2, 4) == Some(false));
    assert(bridge::bridge_ordered_sequence_parts(root, arena)
        == Some((6i32, Seq::<usize>::empty(), None::<usize>, false)));
    let default_kind = score::ScoreNode::Integer(-1i64);
    assert(score::integer_transcript(-1i64).len() == 10);
    assert(score::checked_append(before.transcript, default_kind).is_some());
    bridge::bridge_append_unattached_exact(before, default_kind);
    let (after_default, default_handle) =
        bridge::bridge_append_unattached(before, default_kind).unwrap();
    assert(default_handle == 0);
    let flag_kind = score::ScoreNode::Integer(0i64);
    assert(score::integer_transcript(0i64).len() == 10);
    assert(score::checked_append(after_default.transcript, flag_kind).is_some());
    bridge::bridge_append_unattached_exact(after_default, flag_kind);
    let (after_flag, flag_handle) =
        bridge::bridge_append_unattached(after_default, flag_kind).unwrap();
    assert(flag_handle == 1);
    let children = seq![default_handle] + Seq::<usize>::empty() + seq![flag_handle];
    assert(children == seq![0usize, 1usize]);
    assert(score::child_handles_valid(after_flag.transcript.entries, children));
    let constructor_kind = score::ScoreNode::Integer(6i64);
    assert(score::integer_transcript(6i64).len() == 10);
    assert(score::checked_append(after_flag.transcript, constructor_kind).is_some());
    bridge::bridge_append_unattached_exact(after_flag, constructor_kind);
    let (after_constructor, constructor_handle) =
        bridge::bridge_append_unattached(after_flag, constructor_kind).unwrap();
    assert(constructor_handle == 2);
    let all_children = seq![constructor_handle] + children;
    assert(all_children == seq![2usize, 0usize, 1usize]);
    let entries = after_constructor.transcript.entries;
    assert(entries.len() == 3);
    assert(entries[0].transcript.len() == 10);
    assert(entries[1].transcript.len() == 10);
    assert(entries[2].transcript.len() == 10);
    assert(all_children.drop_first() == seq![0usize, 1usize]);
    assert(seq![0usize, 1usize].drop_first() == seq![1usize]);
    assert(seq![1usize].drop_first() == Seq::<usize>::empty());
    assert(score::children_transcript(entries, Seq::<usize>::empty()) == Seq::<u8>::empty());
    assert(score::children_transcript(entries, seq![1usize]).len() == 10);
    assert(score::children_transcript(entries, seq![0usize, 1usize]).len() == 20);
    assert(score::children_transcript(entries, all_children).len() == 30);
    let node = score::ScoreNode::Node(all_children);
    assert(score::kind_transcript(entries, node).len() == 32);
    assert(score::checked_append(after_constructor.transcript, node).is_some());
    bridge::bridge_append_unattached_exact(after_constructor, node);
    let (after_node, score_handle) =
        bridge::bridge_append_unattached(after_constructor, node).unwrap();
    assert(admission::admission_attach_score_step(
        after_node.admission, score_handle, root.body_id,
    ).is_some());
    assert(bridge::bridge_constructor_step(after_flag, 6i32, children).is_some());
}

/// The class-5 witness is a well-formed EOF prestate of class 5 on which the nonsemantic finish
/// (constructor `i32::MAX`) fires.
pub proof fn finish_witness_nonsemantic_fires()
    ensures
        finish_eof_prestate(finish_witness_class_5()),
        finish_class_5(finish_witness_class_5()),
        finish_candidate_nonsemantic(finish_witness_class_5()).is_some(),
{
    reveal(finish_witness_class_5);
    reveal(finish_candidate_nonsemantic);
    reveal(finish_route);
    let root = finish_witness_root(
        admission::AdmissionFamily::Send, Set::empty(), Seq::empty(), 15, 6,
    );
    assert forall|slot: int| 0 <= slot < root.slot_count as int
        implies root.required.contains(slot) == (exists|tag: u32|
            admission::admission_required_descriptor_slot(root.program_type_id, tag, slot)) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(15, tag, slot) by {}
    }
    assert(admission::admission_frame_wf(root));
    finish_witness_prestate(root);
    assert(!admission::admission_family_complete(root));
    finish_witness_leaf_fires(root, i32::MAX);
}

/// The Var witness is a well-formed EOF prestate of class 6 on which the Var finish fires.
pub proof fn finish_witness_var_fires()
    ensures
        finish_eof_prestate(finish_witness_var()),
        finish_class_6(finish_witness_var()),
        finish_candidate_var(finish_witness_var()).is_some(),
{
    reveal(finish_witness_var);
    reveal(finish_candidate_var);
    reveal(finish_eof_prestate);
    reveal(finish_route);
    bridge::bridge_score_finish_selection_group_6_var_child_before_wf();
    bridge::bridge_score_finish_selection_group_6_var_child_witness();
    let before = finish_witness_var();
    let top = before.admission.frames.last();
    assert(top == bridge::bridge_var_child_witness_child());
    assert(top.family == admission::AdmissionFamily::Other);
    assert(top.program_type_id == 6);
}

/// The owner witness is a well-formed EOF prestate of class 6 on which the EPathMap owner finish
/// fires.
pub proof fn finish_witness_owner_fires()
    ensures
        finish_eof_prestate(finish_witness_owner()),
        finish_class_6(finish_witness_owner()),
        finish_candidate_epathmap_owner(finish_witness_owner()).is_some(),
{
    reveal(finish_witness_owner);
    reveal(finish_candidate_epathmap_owner);
    reveal(finish_eof_prestate);
    reveal(finish_route);
    bridge::bridge_epathmap_owner_score_modeled_witness();
    let before = finish_witness_owner();
    let top = before.admission.frames.last();
    assert(top == before.admission.frames[0]);
    assert(top.family == admission::AdmissionFamily::Other);
    assert(top.program_type_id == 31);
}

/// The EZipper witness is a well-formed EOF prestate of class 6 on which the EZipper finish fires.
pub proof fn finish_witness_ezipper_fires()
    ensures
        finish_eof_prestate(finish_witness_ezipper()),
        finish_class_6(finish_witness_ezipper()),
        finish_candidate_ezipper(finish_witness_ezipper()).is_some(),
{
    reveal(finish_witness_ezipper);
    reveal(finish_candidate_ezipper);
    reveal(finish_eof_prestate);
    reveal(finish_route);
    bridge::bridge_score_finish_selection_group_6_ezipper_witness();
    bridge::bridge_ezipper_witness_child_completes();
    let before = finish_witness_ezipper();
    let top = before.admission.frames.last();
    assert(top.family == admission::AdmissionFamily::Other);
    assert(top.program_type_id == 32);
    assert(top.cursor == top.region_end);
    assert(top.waiting_id.is_none());
    assert(top.nested_started == top.nested_resolved);
    assert(top.score_handle.is_none());
}

/// Non-vacuity of class 1: its witness carries a successful finish.
pub proof fn finish_nonvacuous_class_1()
    ensures
        finish_eof_prestate(finish_witness_class_1()),
        finish_class_1(finish_witness_class_1()),
        finish_candidate_cost_token_stack(finish_witness_class_1()).is_some(),
        bridge::bridge_score_source_step(
            finish_witness_class_1(),
            finish_candidate_cost_token_stack(finish_witness_class_1()).unwrap(),
        ),
{
    finish_witness_cost_token_stack_fires();
    let before = finish_witness_class_1();
    let after = finish_candidate_cost_token_stack(before).unwrap();
    finish_source_step_by_group(before, after);
}

/// Non-vacuity of class 2.
pub proof fn finish_nonvacuous_class_2()
    ensures
        finish_eof_prestate(finish_witness_class_2()),
        finish_class_2(finish_witness_class_2()),
        finish_candidate_absent_specialized(finish_witness_class_2()).is_some(),
        bridge::bridge_score_source_step(
            finish_witness_class_2(),
            finish_candidate_absent_specialized(finish_witness_class_2()).unwrap(),
        ),
{
    finish_witness_absent_specialized_fires();
    let before = finish_witness_class_2();
    let after = finish_candidate_absent_specialized(before).unwrap();
    finish_source_step_by_group(before, after);
}

/// Non-vacuity of class 3.
pub proof fn finish_nonvacuous_class_3()
    ensures
        finish_eof_prestate(finish_witness_class_3()),
        finish_class_3(finish_witness_class_3()),
        finish_candidate_sys_auth_token(finish_witness_class_3()).is_some(),
        bridge::bridge_score_source_step(
            finish_witness_class_3(),
            finish_candidate_sys_auth_token(finish_witness_class_3()).unwrap(),
        ),
{
    finish_witness_sys_auth_token_fires();
    let before = finish_witness_class_3();
    let after = finish_candidate_sys_auth_token(before).unwrap();
    finish_source_step_by_group(before, after);
}

/// Non-vacuity of class 4.
pub proof fn finish_nonvacuous_class_4()
    ensures
        finish_eof_prestate(finish_witness_class_4()),
        finish_class_4(finish_witness_class_4()),
        finish_candidate_ordered_sequence(finish_witness_class_4()).is_some(),
        bridge::bridge_score_source_step(
            finish_witness_class_4(),
            finish_candidate_ordered_sequence(finish_witness_class_4()).unwrap(),
        ),
{
    finish_witness_ordered_sequence_fires();
    let before = finish_witness_class_4();
    let after = finish_candidate_ordered_sequence(before).unwrap();
    finish_source_step_by_group(before, after);
}

/// Non-vacuity of class 5.
pub proof fn finish_nonvacuous_class_5()
    ensures
        finish_eof_prestate(finish_witness_class_5()),
        finish_class_5(finish_witness_class_5()),
        finish_candidate_nonsemantic(finish_witness_class_5()).is_some(),
        bridge::bridge_score_source_step(
            finish_witness_class_5(),
            finish_candidate_nonsemantic(finish_witness_class_5()).unwrap(),
        ),
{
    finish_witness_nonsemantic_fires();
    let before = finish_witness_class_5();
    let after = finish_candidate_nonsemantic(before).unwrap();
    finish_source_step_by_group(before, after);
}

/// Non-vacuity of class 6 for the Var finish.
pub proof fn finish_nonvacuous_class_6_var()
    ensures
        finish_eof_prestate(finish_witness_var()),
        finish_class_6(finish_witness_var()),
        finish_candidate_var(finish_witness_var()).is_some(),
        bridge::bridge_score_source_step(
            finish_witness_var(), finish_candidate_var(finish_witness_var()).unwrap(),
        ),
{
    finish_witness_var_fires();
    let before = finish_witness_var();
    let after = finish_candidate_var(before).unwrap();
    finish_source_step_by_group(before, after);
}

/// Non-vacuity of class 6 for the EPathMap owner finish.
pub proof fn finish_nonvacuous_class_6_owner()
    ensures
        finish_eof_prestate(finish_witness_owner()),
        finish_class_6(finish_witness_owner()),
        finish_candidate_epathmap_owner(finish_witness_owner()).is_some(),
        bridge::bridge_score_source_step(
            finish_witness_owner(),
            finish_candidate_epathmap_owner(finish_witness_owner()).unwrap(),
        ),
{
    finish_witness_owner_fires();
    let before = finish_witness_owner();
    let after = finish_candidate_epathmap_owner(before).unwrap();
    finish_source_step_by_group(before, after);
}

/// Non-vacuity of class 6 for the EZipper finish.
pub proof fn finish_nonvacuous_class_6_ezipper()
    ensures
        finish_eof_prestate(finish_witness_ezipper()),
        finish_class_6(finish_witness_ezipper()),
        finish_candidate_ezipper(finish_witness_ezipper()).is_some(),
        bridge::bridge_score_source_step(
            finish_witness_ezipper(),
            finish_candidate_ezipper(finish_witness_ezipper()).unwrap(),
        ),
{
    finish_witness_ezipper_fires();
    let before = finish_witness_ezipper();
    let after = finish_candidate_ezipper(before).unwrap();
    finish_source_step_by_group(before, after);
}

}
