//! Universal coverage of the deterministic no-score Message child EOF route.
//!
//! Source obligation: item 3 of the remaining source-audit obligations in
//! `docs/design/pathmap/rpb3-admission-source-traceability.md`, which extends the
//! no-score child EOF route to every successful no-score production summary.
//!
//! The route under audit is the runtime selector
//! `runtime_source::runtime_noscore_child_kind` together with the EOF arm of
//! `runtime_source::runtime_message_driver_step`, including its EMap and EZipper arms.
//! A Message child reaches the no-score route when the model's Complete step accepts it
//! and the summary that Complete pushes (`concrete_derived_summary_with_arena`) carries
//! no score. In production these are the children whose `MessageFrame::finish` returns
//! a semantic summary without a score, for example `reduce_emap` on a map with a pair
//! that lacks its key or value, or `reduce_ezipper` without its PathMap.
//!
//! The case split `nsc_case_of_program` names fifteen classes, keyed by the
//! child's generated program id.
//!
//! * Theorem C, `nsc_case_split_total`: every successful no-score Message child
//!   EOF falls into exactly one class, with the stated summary shape.
//! * Theorem R, `nsc_route_selects_every_case` and
//!   `nsc_every_case_reaches_parent_accept`: for every class the selector returns the
//!   pushed summary and the runtime EOF step is the model Complete step.
//! * Theorem X, `nsc_noscore_route_excludes_score_route`: whenever the no-score
//!   selector selects a kind, the score-finish selector proposes nothing and no score
//!   source step exists, so the driver's "no-score first, else score" order never
//!   hides a score finish. Lemma A, `nsc_quiet_groups_exclude_score_route`, joins the
//!   six selection groups to the source-step relation. Theorem S,
//!   `nsc_emap_ezipper_score_selector_none`, is the EMap and EZipper instance.
//! * `nsc_eof_coverage_universal`: the headline, Theorems C, R and X for every
//!   successful no-score Message child EOF.
//! * `nsc_selected_kind_is_pushed_summary`: whatever the selector picks on a
//!   successful Complete step is the pushed no-score summary.
//! * Theorems T1 and T2, `nsc_descriptor_children_classified` and
//!   `nsc_class_arms_are_descriptor_edges`: the case split is exhaustive over the
//!   Message child edges of the generated descriptor table, arm by arm.
//! * Non-vacuity: `nsc_inhabited_*_class` and the routed witnesses
//!   `nsc_ezipper_absent_routed_witness` and `nsc_emap_incomplete_routed_witness`.
//! * `nsc_source_noscore_route_on_emap_ezipper`: the source-level no-score route
//!   `runtime_source::source_noscore_message_eof_child_accept` agrees with the runtime
//!   route on the EMap and EZipper classes.
use vstd::prelude::*;

#[path = "rpb3_generated_descriptor_projection.rs"]
mod rpb3_generated_descriptor_projection;
use rpb3_generated_descriptor_projection as table;

use crate::{
    rpb3_admission_cursor as admission,
    rpb3_runtime_frame_source_refinement as runtime_source,
    rpb3_score_admission_bridge as bridge,
};

verus! {

/// Classes of successful no-score Message children at EOF.
pub enum NoScoreChildEofCase {
    ReceiveBindSourceAbsent,
    MatchCaseRequiredAbsent,
    BigRationalLeaf,
    FixedPointLeaf,
    UnitLeaf,
    VarRefLeaf,
    ConnectiveBodyLeaf,
    BytesLeaf,
    EVarAbsent,
    ExprStructuralMissingPar,
    EMethodMissingTarget,
    KeyValuePairUnderEMap,
    EZipperPathMapAbsent,
    EMapIncomplete,
    DescriptorOpaque,
}

/// The case split, keyed by the child's generated program id.
pub open spec fn nsc_case_of_program(program: u16) -> Option<NoScoreChildEofCase> {
    if 36 <= program && program <= 54 {
        Some(NoScoreChildEofCase::ExprStructuralMissingPar)
    } else {
        match program {
            3 | 14 | 17 => Some(NoScoreChildEofCase::DescriptorOpaque),
            7 | 64 => Some(NoScoreChildEofCase::UnitLeaf),
            16 => Some(NoScoreChildEofCase::ReceiveBindSourceAbsent),
            21 => Some(NoScoreChildEofCase::MatchCaseRequiredAbsent),
            25 => Some(NoScoreChildEofCase::BigRationalLeaf),
            26 => Some(NoScoreChildEofCase::FixedPointLeaf),
            30 => Some(NoScoreChildEofCase::EMapIncomplete),
            32 => Some(NoScoreChildEofCase::EZipperPathMapAbsent),
            33 => Some(NoScoreChildEofCase::EMethodMissingTarget),
            34 => Some(NoScoreChildEofCase::KeyValuePairUnderEMap),
            35 => Some(NoScoreChildEofCase::EVarAbsent),
            56 => Some(NoScoreChildEofCase::VarRefLeaf),
            57 => Some(NoScoreChildEofCase::ConnectiveBodyLeaf),
            61 | 62 | 63 => Some(NoScoreChildEofCase::BytesLeaf),
            _ => None,
        }
    }
}

pub open spec fn nsc_child(
    before: runtime_source::RuntimeFrameImage,
) -> admission::AdmissionFrameState {
    before.score.admission.frames.last()
}

pub open spec fn nsc_parent(
    before: runtime_source::RuntimeFrameImage,
) -> admission::AdmissionFrameState {
    before.score.admission.frames[before.score.admission.frames.len() - 2]
}

/// The summary the model's Complete step derives and pushes into the parent.
pub open spec fn nsc_summary(
    before: runtime_source::RuntimeFrameImage,
) -> Option<admission::AdmissionReturnedKind> {
    admission::concrete_derived_summary_with_arena(
        nsc_parent(before), nsc_child(before), before.score.admission.arena,
    )
}

/// A successful no-score Message child EOF prestate.
pub open spec fn nsc_eof_prestate(before: runtime_source::RuntimeFrameImage) -> bool {
    &&& runtime_source::runtime_frame_image_wf(before)
    &&& before.score.admission.frames.len() > 1
    &&& nsc_child(before).frame_kind == 1
    &&& runtime_source::runtime_pre_pop_child_accept(before).is_some()
    &&& match nsc_summary(before) {
        Some(kind) => !admission::admission_summary_has_score(kind),
        None => false,
    }
}

/// Per-class shape of the derived summary and of the parent arm.
pub open spec fn nsc_case_shape(
    case: NoScoreChildEofCase,
    parent: admission::AdmissionFrameState,
    child: admission::AdmissionFrameState,
    kind: admission::AdmissionReturnedKind,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> bool {
    match case {
        NoScoreChildEofCase::ReceiveBindSourceAbsent =>
            child.family == admission::AdmissionFamily::ReceiveBindFrame
                && kind == (admission::AdmissionReturnedKind::ReceiveBind {
                    source_present: false,
                })
                && !admission::admission_family_complete(child),
        NoScoreChildEofCase::MatchCaseRequiredAbsent =>
            child.family == admission::AdmissionFamily::MatchCaseFrame
                && match kind {
                    admission::AdmissionReturnedKind::MatchCase { required_present, .. } =>
                        !required_present,
                    _ => false,
                }
                && !admission::admission_family_complete(child),
        NoScoreChildEofCase::BigRationalLeaf =>
            child.family == admission::AdmissionFamily::BigRationalFrame
                && match kind {
                    admission::AdmissionReturnedKind::BigRational { .. } => true,
                    _ => false,
                },
        NoScoreChildEofCase::FixedPointLeaf =>
            child.family == admission::AdmissionFamily::FixedPointFrame
                && match kind {
                    admission::AdmissionReturnedKind::FixedPoint { .. } => true,
                    _ => false,
                },
        NoScoreChildEofCase::UnitLeaf =>
            child.family == admission::AdmissionFamily::Other
                && kind == admission::AdmissionReturnedKind::Unit,
        NoScoreChildEofCase::VarRefLeaf =>
            child.family == admission::AdmissionFamily::Other
                && match kind {
                    admission::AdmissionReturnedKind::VarRef { .. } => true,
                    _ => false,
                },
        NoScoreChildEofCase::ConnectiveBodyLeaf =>
            child.family == admission::AdmissionFamily::Other
                && match kind {
                    admission::AdmissionReturnedKind::ConnectiveBody { .. } => true,
                    _ => false,
                },
        NoScoreChildEofCase::BytesLeaf =>
            child.family == admission::AdmissionFamily::Other
                && match kind {
                    admission::AdmissionReturnedKind::Bytes { .. } => true,
                    _ => false,
                },
        NoScoreChildEofCase::EVarAbsent =>
            child.family == admission::AdmissionFamily::EVarFrame
                && parent.family == admission::AdmissionFamily::Expr
                && parent.waiting_tag == Some(19u32)
                && kind == runtime_source::runtime_noscore_absent_expr_payload()
                && !admission::admission_family_complete(child),
        NoScoreChildEofCase::ExprStructuralMissingPar =>
            child.family == admission::AdmissionFamily::Other
                && parent.family == admission::AdmissionFamily::Expr
                && parent.waiting_slot == Some(0usize)
                && kind == runtime_source::runtime_noscore_absent_expr_payload()
                && admission::admission_expr_structural_payload(parent, child) == Some(kind)
                && (!admission::admission_returned_par(child, 0)
                    || (bridge::bridge_expr_structural_constructor(child.program_type_id)
                        .is_some()
                        && bridge::bridge_expr_structural_constructor(child.program_type_id)
                            .unwrap().1
                        && !admission::admission_returned_par(child, 1))),
        NoScoreChildEofCase::EMethodMissingTarget =>
            child.family == admission::AdmissionFamily::Other
                && parent.family == admission::AdmissionFamily::Expr
                && parent.waiting_slot == Some(0usize)
                && parent.waiting_tag == Some(24u32)
                && kind == runtime_source::runtime_noscore_absent_expr_payload()
                && admission::admission_expr_structural_payload(parent, child) == Some(kind)
                && !admission::admission_returned_par(child, 1),
        NoScoreChildEofCase::KeyValuePairUnderEMap =>
            child.family == admission::AdmissionFamily::Other
                && parent.family == admission::AdmissionFamily::Other
                && parent.program_type_id == 30
                && parent.frame_kind == 1
                && parent.waiting_slot == Some(0usize)
                && parent.waiting_tag == Some(1u32)
                && match kind {
                    admission::AdmissionReturnedKind::KeyValuePair { key, value_par } =>
                        admission::admission_key_value_pair_bindings(key, value_par, arena),
                    _ => false,
                },
        NoScoreChildEofCase::EZipperPathMapAbsent =>
            child.family == admission::AdmissionFamily::Other
                && parent.family == admission::AdmissionFamily::Expr
                && parent.waiting_slot == Some(0usize)
                && parent.waiting_tag == Some(33u32)
                && kind == runtime_source::runtime_noscore_absent_expr_payload(),
        NoScoreChildEofCase::EMapIncomplete =>
            child.family == admission::AdmissionFamily::Other
                && parent.family == admission::AdmissionFamily::Expr
                && parent.waiting_slot == Some(0usize)
                && parent.waiting_tag == Some(23u32)
                && match kind {
                    admission::AdmissionReturnedKind::ExprPayload {
                        score_present, canonical_path_stable, ..
                    } => !score_present && !canonical_path_stable,
                    _ => false,
                },
        NoScoreChildEofCase::DescriptorOpaque =>
            child.family == admission::AdmissionFamily::Other
                && kind == admission::AdmissionReturnedKind::Other
                && admission::admission_opaque_child_edge(parent, child),
    }
}

/// Facts that a successful model Complete step establishes for the top child.
pub proof fn nsc_complete_step_facts(before: runtime_source::RuntimeFrameImage)
    requires
        runtime_source::runtime_frame_image_wf(before),
        before.score.admission.frames.len() > 1,
        runtime_source::runtime_pre_pop_child_accept(before).is_some(),
    ensures
        nsc_summary(before).is_some(),
        nsc_parent(before).frame_kind == 1,
        nsc_child(before).return_id.is_some(),
        nsc_parent(before).waiting_id == nsc_child(before).return_id,
        nsc_child(before).cursor == nsc_child(before).region_end,
        nsc_child(before).waiting_id.is_none(),
        admission::admission_frame_wf(nsc_child(before)),
        admission::admission_frame_wf(nsc_parent(before)),
        admission::admission_child_summary_agrees(
            nsc_child(before), nsc_summary(before).unwrap(), before.score.admission.arena,
        ),
        admission::admission_arm_child_summary_agrees(
            nsc_parent(before), nsc_child(before), nsc_summary(before).unwrap(),
            before.score.admission.arena,
        ),
        nsc_parent(before).waiting_slot.is_some(),
        nsc_parent(before).waiting_tag.is_some(),
{
    reveal(runtime_source::runtime_pre_pop_child_accept);
    let state = before.score.admission;
    let child = state.frames[state.frames.len() - 1];
    let parent = state.frames[state.frames.len() - 2];
    assert(admission::admission_state_wf(state));
    assert(admission::admission_frame_wf(child));
    assert(admission::admission_frame_wf(parent));
    assert(admission::concrete_message_complete_child(state).is_some());
    let summary = admission::concrete_derived_summary_with_arena(parent, child, state.arena).unwrap();
    assert(admission::admission_complete_child_step(
        state, child.return_id.unwrap(), child.frame_kind, summary,
    ).is_some());
}

/// Branch B0 of `concrete_derived_summary_with_arena`: an Expr parent waiting on
/// ESet (tag 22) or EMap (tag 23). ESet always carries a score, and the
/// structural payload has no arm for tags 22 and 23, so a no-score summary here
/// is exactly an incomplete EMap.
pub proof fn nsc_classify_set_map_arm(
    parent: admission::AdmissionFrameState,
    child: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
    kind: admission::AdmissionReturnedKind,
)
    requires
        parent.family == admission::AdmissionFamily::Expr,
        parent.waiting_slot == Some(0usize),
        parent.waiting_tag == Some(22u32) || parent.waiting_tag == Some(23u32),
        parent.program_type_id == 24,
        admission::concrete_derived_summary_with_arena(parent, child, arena) == Some(kind),
        !admission::admission_summary_has_score(kind),
    ensures
        parent.waiting_tag == Some(23u32),
        child.family == admission::AdmissionFamily::Other,
        child.frame_kind == 1,
        child.program_type_id == 30,
        nsc_case_of_program(child.program_type_id) == Some(NoScoreChildEofCase::EMapIncomplete),
        nsc_case_shape(NoScoreChildEofCase::EMapIncomplete, parent, child, kind, arena),
{
    assert(admission::concrete_derived_summary_with_arena(parent, child, arena)
        == admission::admission_expr_structural_payload_with_arena(parent, child, arena));
    if !(child.family == admission::AdmissionFamily::Other && child.frame_kind == 1
        && (child.program_type_id == 29 || child.program_type_id == 30))
    {
        assert(admission::admission_expr_structural_payload(parent, child).is_none());
    } else if child.program_type_id == 29 {
        if parent.waiting_tag == Some(23u32) {
            assert(admission::admission_expr_structural_payload(parent, child).is_none());
        }
    } else if parent.waiting_tag == Some(22u32) {
        assert(admission::admission_expr_structural_payload(parent, child).is_none());
    }
}

/// Branches B1-B3: a Par parent (Component), a Par child, and the EZipper
/// PathMap arm only produce scored summaries, so a no-score summary takes a later
/// branch, where the arena-free and arena-aware summaries coincide.
pub proof fn nsc_classify_scored_prefix(
    parent: admission::AdmissionFrameState,
    child: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
    kind: admission::AdmissionReturnedKind,
)
    requires
        !(parent.family == admission::AdmissionFamily::Expr
            && parent.waiting_slot == Some(0usize)
            && (parent.waiting_tag == Some(22u32) || parent.waiting_tag == Some(23u32))
            && parent.program_type_id == 24),
        admission::concrete_derived_summary_with_arena(parent, child, arena) == Some(kind),
        !admission::admission_summary_has_score(kind),
    ensures
        admission::concrete_derived_summary(parent, child) == Some(kind),
        parent.program_type_id != 1,
        !(child.frame_kind == 1 && child.program_type_id == 1),
        !(parent.family == admission::AdmissionFamily::Other
            && parent.program_type_id == 32
            && parent.waiting_slot == Some(0usize)
            && parent.waiting_tag == Some(1u32)),
{
    assert(admission::concrete_derived_summary_with_arena(parent, child, arena)
        == admission::concrete_derived_summary(parent, child));
}

/// Branch B4: the structural Expr payload. Tags 20, 21 (EList, ETuple) and 32
/// (EPathMap) always carry a score; every other arm names its child program
/// through the generated descriptor of Expr.
pub proof fn nsc_classify_structural_arm(
    parent: admission::AdmissionFrameState,
    child: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
    kind: admission::AdmissionReturnedKind,
)
    requires
        parent.family == admission::AdmissionFamily::Expr,
        parent.program_type_id == 24,
        parent.waiting_slot == Some(0usize),
        parent.waiting_tag.is_some(),
        runtime_source::runtime_noscore_expr_structural_tag(parent.waiting_tag.unwrap()),
        admission::admission_expr_structural_payload(parent, child) == Some(kind),
        !admission::admission_summary_has_score(kind),
    ensures
        child.family == admission::AdmissionFamily::Other,
        child.frame_kind == 1,
        match nsc_case_of_program(child.program_type_id) {
            Some(case) => nsc_case_shape(case, parent, child, kind, arena)
                && (case == NoScoreChildEofCase::ExprStructuralMissingPar
                    || case == NoScoreChildEofCase::EMethodMissingTarget
                    || case == NoScoreChildEofCase::EZipperPathMapAbsent),
            None => false,
        },
{
    let tag = parent.waiting_tag.unwrap();
    if tag == 33 {
        assert(child.program_type_id == 32);
    } else if tag == 24 {
        assert(child.program_type_id == 33);
    } else if tag == 5 || tag == 6 {
        assert(36 <= child.program_type_id && child.program_type_id <= 37);
    } else {
        assert((7 <= tag && tag <= 18) || (27 <= tag && tag <= 31)
            || tag == 20 || tag == 21 || tag == 32);
        if (7 <= tag && tag <= 18) || (27 <= tag && tag <= 31) {
            assert(38 <= child.program_type_id && child.program_type_id <= 54);
            assert(bridge::bridge_expr_structural_constructor(child.program_type_id).is_some());
            assert(bridge::bridge_expr_structural_constructor(child.program_type_id).unwrap().1);
        }
    }
}

/// Branch B5: the per-family arm of `concrete_derived_summary`.
pub proof fn nsc_classify_family_arm(
    parent: admission::AdmissionFrameState,
    child: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
    kind: admission::AdmissionReturnedKind,
)
    requires
        admission::admission_frame_wf(child),
        parent.frame_kind == 1,
        child.frame_kind == 1,
        parent.program_type_id != 1,
        child.program_type_id != 1,
        !(parent.family == admission::AdmissionFamily::Other
            && parent.program_type_id == 32
            && parent.waiting_slot == Some(0usize)
            && parent.waiting_tag == Some(1u32)),
        !(parent.family == admission::AdmissionFamily::Expr
            && parent.waiting_slot == Some(0usize)
            && match parent.waiting_tag {
                Some(tag) => runtime_source::runtime_noscore_expr_structural_tag(tag),
                None => false,
            }),
        admission::concrete_derived_summary(parent, child) == Some(kind),
        admission::admission_child_summary_agrees(child, kind, arena),
        !admission::admission_summary_has_score(kind),
    ensures
        match nsc_case_of_program(child.program_type_id) {
            Some(case) => nsc_case_shape(case, parent, child, kind, arena),
            None => false,
        },
{
    assert(admission::concrete_derived_summary(parent, child) == match child.family {
        admission::AdmissionFamily::ReceiveBindFrame => Some(
            admission::AdmissionReturnedKind::ReceiveBind {
                source_present: admission::admission_returned_par(child, 1),
            }),
        admission::AdmissionFamily::MatchCaseFrame => Some(
            admission::AdmissionReturnedKind::MatchCase {
                required_present: admission::admission_returned_par(child, 0)
                    && admission::admission_returned_par(child, 1),
                canonical_fixed_point: !admission::admission_returned_default_par(child, 3),
            }),
        admission::AdmissionFamily::CostSignatureFrame
        | admission::AdmissionFamily::CostRegionFrame =>
            Some(admission::AdmissionReturnedKind::CostScore),
        admission::AdmissionFamily::BigRationalFrame =>
            match admission::admission_big_rational_fields(child) {
                Some((numerator, denominator)) => Some(
                    admission::AdmissionReturnedKind::BigRational { numerator, denominator }),
                None => None,
            },
        admission::AdmissionFamily::FixedPointFrame =>
            match admission::admission_fixed_point_fields(child) {
                Some((unscaled, scale)) => Some(
                    admission::AdmissionReturnedKind::FixedPoint { unscaled, scale }),
                None => None,
            },
        admission::AdmissionFamily::EVarFrame if parent.family
            == admission::AdmissionFamily::Expr && parent.waiting_tag == Some(19u32) => Some(
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: admission::admission_returned_var(child, 0),
                canonical_fixed_point: true,
                canonical_path_stable: false,
            }),
        admission::AdmissionFamily::Other if child.frame_kind == 1
            && child.program_type_id == 34
            && parent.family == admission::AdmissionFamily::Other
            && parent.program_type_id == 30
            && parent.waiting_slot == Some(0usize)
            && parent.waiting_tag == Some(1u32) =>
            match admission::admission_key_value_pair_fields(child) {
                Some((key, value_par)) => Some(
                    admission::AdmissionReturnedKind::KeyValuePair { key, value_par }),
                None => None,
            },
        admission::AdmissionFamily::Other if admission::admission_opaque_child_edge(
            parent, child,
        ) => Some(admission::AdmissionReturnedKind::Other),
        admission::AdmissionFamily::Other if child.frame_kind == 2
            && parent.family == admission::AdmissionFamily::New
            && parent.waiting_slot == Some(3usize)
            && parent.waiting_tag == Some(4u32)
            && admission::admission_map_summary_complete(child) => Some(
            admission::AdmissionReturnedKind::Map {
                key_bytes: admission::admission_map_key_bytes(child),
                value_score_handle: admission::admission_map_value_score_handle(child),
            }),
        admission::AdmissionFamily::Other if child.frame_kind == 1 =>
            match child.program_type_id {
                6 => Some(admission::AdmissionReturnedKind::VarScore),
                7 | 64 => Some(admission::AdmissionReturnedKind::Unit),
                10 | 11 => Some(admission::AdmissionReturnedKind::CostScore),
                56 => match admission::admission_var_ref_fields(child) {
                    Some((index, depth)) => Some(
                        admission::AdmissionReturnedKind::VarRef { index, depth }),
                    None => None,
                },
                57 => match admission::admission_connective_body_handles(child) {
                    Some(par_handles) => Some(
                        admission::AdmissionReturnedKind::ConnectiveBody { par_handles }),
                    None => None,
                },
                61 | 62 | 63 => match admission::admission_bytes_leaf_payload(child) {
                    Some(value) => Some(admission::AdmissionReturnedKind::Bytes { value }),
                    None => None,
                },
                _ => None,
            },
        _ => None,
    });
    if child.family == admission::AdmissionFamily::Other
        && admission::admission_opaque_child_edge(parent, child)
        && !(child.program_type_id == 34
            && parent.family == admission::AdmissionFamily::Other
            && parent.program_type_id == 30
            && parent.waiting_slot == Some(0usize)
            && parent.waiting_tag == Some(1u32))
    {
        assert(child.program_type_id == 3 || child.program_type_id == 14
            || child.program_type_id == 17);
    }
}

/// Branch B0 of the arena-aware summary (Expr parent waiting on ESet or EMap).
pub open spec fn nsc_set_map_arm(parent: admission::AdmissionFrameState) -> bool {
    parent.family == admission::AdmissionFamily::Expr
        && parent.waiting_slot == Some(0usize)
        && (parent.waiting_tag == Some(22u32) || parent.waiting_tag == Some(23u32))
        && parent.program_type_id == 24
}

/// Branch B4 of the arena-free summary (Expr parent waiting on a structural arm).
pub open spec fn nsc_structural_arm(parent: admission::AdmissionFrameState) -> bool {
    parent.family == admission::AdmissionFamily::Expr
        && parent.waiting_slot == Some(0usize)
        && match parent.waiting_tag {
            Some(tag) => runtime_source::runtime_noscore_expr_structural_tag(tag),
            None => false,
        }
}

/// Theorem C, no gap in the case split: every successful no-score Message child
/// EOF has a class, given by its generated program id, and the derived summary
/// and parent arm have that class's shape. EOF position, the absent waiting
/// child, and the absent score handle follow from the successful Complete step.
pub proof fn nsc_case_split_total(before: runtime_source::RuntimeFrameImage)
    requires nsc_eof_prestate(before),
    ensures
        nsc_child(before).cursor == nsc_child(before).region_end,
        nsc_child(before).waiting_id.is_none(),
        nsc_child(before).score_handle.is_none(),
        match nsc_case_of_program(nsc_child(before).program_type_id) {
            Some(case) => nsc_case_shape(
                case, nsc_parent(before), nsc_child(before), nsc_summary(before).unwrap(),
                before.score.admission.arena,
            ),
            None => false,
        },
{
    nsc_complete_step_facts(before);
    let parent = nsc_parent(before);
    let child = nsc_child(before);
    let arena = before.score.admission.arena;
    let kind = nsc_summary(before).unwrap();
    if nsc_set_map_arm(parent) {
        nsc_classify_set_map_arm(parent, child, arena, kind);
    } else {
        nsc_classify_scored_prefix(parent, child, arena, kind);
        if nsc_structural_arm(parent) {
            assert(parent.program_type_id == 24);
            assert(admission::concrete_derived_summary(parent, child)
                == admission::admission_expr_structural_payload(parent, child));
            nsc_classify_structural_arm(parent, child, arena, kind);
        } else {
            nsc_classify_family_arm(parent, child, arena, kind);
        }
    }
}

/// Theorem R, selector side: for every class the selector `runtime_noscore_child_kind`
/// returns the summary the model Complete step pushes. An incomplete EMap reaches the
/// selector's EMap arm, whose arena-free summary is absent; every other class is selected
/// from the arena-free summary, which equals the pushed one off the ESet and EMap arms.
pub proof fn nsc_route_selects_every_case(before: runtime_source::RuntimeFrameImage)
    requires nsc_eof_prestate(before),
    ensures runtime_source::runtime_noscore_child_kind(before) == nsc_summary(before),
{
    nsc_complete_step_facts(before);
    nsc_case_split_total(before);
    let parent = nsc_parent(before);
    let child = nsc_child(before);
    let arena = before.score.admission.arena;
    let kind = nsc_summary(before).unwrap();
    if nsc_set_map_arm(parent) {
        nsc_classify_set_map_arm(parent, child, arena, kind);
        assert(admission::concrete_derived_summary(parent, child).is_none());
    } else {
        nsc_classify_scored_prefix(parent, child, arena, kind);
        assert(admission::concrete_derived_summary(parent, child) == Some(kind));
        if parent.family == admission::AdmissionFamily::Expr {
            assert(parent.program_type_id == 24);
            assert(parent.waiting_slot == Some(0usize));
        }
    }
}

/// Group 1 of the score-finish selection keeps NoCandidate only if none of its
/// eight steps applies: one applicable step makes the selection Unique, and a
/// later step can only keep it Unique or make it Ambiguous.
pub proof fn nsc_quiet_group_one_steps(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_score_finish_selection_group_1(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures
        bridge::bridge_raw_par_step(before).is_none(),
        bridge::bridge_cost_atom_absent_step(before).is_none(),
        bridge::bridge_cost_atom_bytes_step(before).is_none(),
        bridge::bridge_cost_bound_step(before).is_none(),
        bridge::bridge_cost_atom_bound_forward_step(before).is_none(),
        bridge::bridge_cost_signature_step(before).is_none(),
        bridge::bridge_cost_region_step(before).is_none(),
        bridge::bridge_cost_token_stack_step(before).is_none(),
{
    reveal(bridge::bridge_score_finish_selection_group_1);
}

/// Group 2 keeps NoCandidate only if none of its eight steps applies.
pub proof fn nsc_quiet_group_two_steps(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_score_finish_selection_group_2(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures
        bridge::bridge_bundle_step(before).is_none(),
        bridge::bridge_if_step(before).is_none(),
        bridge::bridge_send_step(before).is_none(),
        bridge::bridge_match_step(before).is_none(),
        bridge::bridge_absent_specialized_constructor_step(before).is_none(),
        bridge::bridge_absent_connective_step(before).is_none(),
        bridge::bridge_receive_bind_step(before).is_none(),
        bridge::bridge_match_case_step(before).is_none(),
{
    reveal(bridge::bridge_score_finish_selection_group_2);
}

/// Group 3 keeps NoCandidate only if none of its eight steps applies.
pub proof fn nsc_quiet_group_three_steps(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_score_finish_selection_group_3(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures
        bridge::bridge_bool_oneof_step(before).is_none(),
        bridge::bridge_expr_atom_step(before).is_none(),
        bridge::bridge_expr_numeric_step(before).is_none(),
        bridge::bridge_expr_big_rational_step(before).is_none(),
        bridge::bridge_expr_fixed_point_step(before).is_none(),
        bridge::bridge_sys_auth_token_step(before).is_none(),
        bridge::bridge_new_empty_collections_step(before).is_none(),
        bridge::bridge_new_one_injection_step(before).is_none(),
{
    reveal(bridge::bridge_score_finish_selection_group_3);
}

/// Group 4 keeps NoCandidate only if none of its nine steps applies.
pub proof fn nsc_quiet_group_four_steps(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_score_finish_selection_group_4(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures
        bridge::bridge_new_one_absent_injection_step(before).is_none(),
        bridge::bridge_new_one_uri_step(before).is_none(),
        bridge::bridge_new_collections_step(before).is_none(),
        bridge::bridge_expr_payload_forward_step(before).is_none(),
        bridge::bridge_expr_structural_step(before).is_none(),
        bridge::bridge_evar_step(before).is_none(),
        bridge::bridge_ordered_sequence_step(before).is_none(),
        bridge::bridge_emethod_step(before).is_none(),
        bridge::bridge_emap_finish_step(before).is_none(),
{
    reveal(bridge::bridge_score_finish_selection_group_4);
}

/// Group 5 keeps NoCandidate only if none of its seven steps applies.
pub proof fn nsc_quiet_group_five_steps(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_score_finish_selection_group_5(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures
        bridge::bridge_connective_not_step(before).is_none(),
        bridge::bridge_connective_varref_step(before).is_none(),
        bridge::bridge_connective_collection_step(before).is_none(),
        bridge::bridge_unforgeable_bytes_step(before).is_none(),
        bridge::bridge_receive_present_condition_step(before).is_none(),
        bridge::bridge_receive_absent_condition_step(before).is_none(),
        bridge::bridge_nonsemantic_step(before).is_none(),
{
    reveal(bridge::bridge_score_finish_selection_group_5);
}

/// Group 6 (the Var, EPathMap owner and EZipper candidates, programs 6, 31 and 32)
/// keeps NoCandidate only if none of its three steps applies.
pub proof fn nsc_quiet_group_six_steps(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_score_finish_selection_group_6(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures
        bridge::bridge_var_score_step(before).is_none(),
        bridge::bridge_epathmap_owner_score_step(before).is_none(),
        bridge::bridge_ezipper_score_step(before).is_none(),
{
    reveal(bridge::bridge_score_finish_selection_group_6);
}

/// Lemma A, the join of the two EOF routes: when all six selection groups keep
/// NoCandidate, none of the forty-three score steps applies, so no score source step
/// exists and the score-finish selector proposes nothing.
pub proof fn nsc_quiet_groups_exclude_score_route(before: bridge::ScoreAdmissionBridge)
    requires
        bridge::bridge_score_finish_selection_group_1(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_2(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_3(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_4(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_5(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_6(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
    ensures
        bridge::bridge_score_finish_selected(before).is_none(),
        forall|after: bridge::ScoreAdmissionBridge|
            !bridge::bridge_score_source_step(before, after),
{
    nsc_quiet_group_one_steps(before);
    nsc_quiet_group_two_steps(before);
    nsc_quiet_group_three_steps(before);
    nsc_quiet_group_four_steps(before);
    nsc_quiet_group_five_steps(before);
    nsc_quiet_group_six_steps(before);
    reveal(bridge::bridge_score_finish_selected);
    assert forall|after: bridge::ScoreAdmissionBridge|
        !bridge::bridge_score_source_step(before, after) by {
        reveal(bridge::bridge_score_source_step_group_6);
    }
}

// Disabled: false with group 6, a scored EZipper top selects Unique; see the same-name lemma below.
// pub proof fn nsc_emap_ezipper_score_selector_none(before: bridge::ScoreAdmissionBridge)
//     requires
//         before.admission.frames.len() > 0,
//         before.admission.frames.last().family == admission::AdmissionFamily::Other,
//         before.admission.frames.last().frame_kind == 1,
//         before.admission.frames.last().program_type_id == 30
//             || before.admission.frames.last().program_type_id == 32,
//         bridge::bridge_emap_finish_step(before).is_none(),
//     ensures
//         bridge::bridge_score_finish_selected(before).is_none(),
//         forall|after: bridge::ScoreAdmissionBridge|
//             !bridge::bridge_score_source_step(before, after),
// {
//     runtime_source::runtime_emap_ezipper_score_groups_quiet(before);
//     nsc_quiet_groups_exclude_score_route(before);
// }

/// Theorem S: an EMap (30) or EZipper (32) Message top frame whose EMap finish step and
/// EZipper score step, the only score steps keyed on programs 30 and 32, are absent has no
/// score source step, and the score-finish selector proposes nothing.
pub proof fn nsc_emap_ezipper_score_selector_none(before: bridge::ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        before.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.admission.frames.last().frame_kind == 1,
        before.admission.frames.last().program_type_id == 30
            || before.admission.frames.last().program_type_id == 32,
        bridge::bridge_emap_finish_step(before).is_none(),
        bridge::bridge_ezipper_score_step(before).is_none(),
    ensures
        bridge::bridge_score_finish_selected(before).is_none(),
        forall|after: bridge::ScoreAdmissionBridge|
            !bridge::bridge_score_source_step(before, after),
{
    runtime_source::runtime_emap_ezipper_score_groups_quiet(before);
    nsc_quiet_groups_exclude_score_route(before);
}

/// Every class the selector picks, other than ReceiveBind, MatchCase, EMap and
/// EZipper, has a top frame of the bridge's structural no-score shape.
pub proof fn nsc_selected_structural_frame(before: runtime_source::RuntimeFrameImage)
    requires
        runtime_source::runtime_noscore_child_kind(before).is_some(),
        !bridge::bridge_no_score_child_frame(before.score),
        !(before.score.admission.frames.last().family == admission::AdmissionFamily::Other
            && (before.score.admission.frames.last().program_type_id == 30
                || before.score.admission.frames.last().program_type_id == 32)),
    ensures bridge::bridge_structural_no_score_child_frame(before.score),
{
    let child = before.score.admission.frames.last();
    if bridge::bridge_emethod_missing_target_frame(before.score) {
    } else if bridge::bridge_expr_structural_missing_par_frame(before.score) {
    } else if !bridge::bridge_structural_no_score_child_frame(before.score) {
        if child.family == admission::AdmissionFamily::Other {
            assert(child.frame_kind == 1);
            assert(child.score_handle.is_none());
            assert(child.program_type_id == 3 || child.program_type_id == 14
                || child.program_type_id == 17 || child.program_type_id == 34);
        } else {
            assert(child.family == admission::AdmissionFamily::EVarFrame);
            assert(!admission::admission_returned_var(child, 0));
        }
    }
}

/// Theorem X, exclusivity of the two EOF routes for all fifteen classes: whenever the
/// no-score selector selects a kind, the six score-finish groups keep NoCandidate,
/// so no score source step exists and the score-finish selector proposes nothing. The
/// driver's "no-score first, else score" order therefore never hides a score finish.
pub proof fn nsc_noscore_route_excludes_score_route(before: runtime_source::RuntimeFrameImage)
    requires runtime_source::runtime_noscore_child_kind(before).is_some(),
    ensures
        bridge::bridge_score_finish_selected(before.score).is_none(),
        forall|after: bridge::ScoreAdmissionBridge|
            !bridge::bridge_score_source_step(before.score, after),
{
    let empty = bridge::BridgeScoreFinishSelection::NoCandidate;
    let child = before.score.admission.frames.last();
    if bridge::bridge_no_score_child_frame(before.score) {
        bridge::bridge_no_score_child_group_1_empty(before.score);
        bridge::bridge_no_score_child_group_2_empty(before.score);
        bridge::bridge_no_score_child_group_3_empty(before.score, empty);
        bridge::bridge_no_score_child_group_4_empty(before.score, empty);
        bridge::bridge_no_score_child_group_5_empty(before.score, empty);
        bridge::bridge_no_score_child_group_6_empty(before.score, empty);
    } else if child.family == admission::AdmissionFamily::Other
        && (child.program_type_id == 30 || child.program_type_id == 32)
    {
        runtime_source::runtime_noscore_emap_ezipper_selected_quiet(before);
    } else {
        nsc_selected_structural_frame(before);
        bridge::bridge_structural_no_score_group_1_empty(before.score);
        bridge::bridge_structural_no_score_group_2_empty(before.score);
        bridge::bridge_structural_no_score_group_3_empty(before.score, empty);
        bridge::bridge_structural_no_score_group_4_empty(before.score, empty);
        bridge::bridge_structural_no_score_group_5_empty(before.score, empty);
        bridge::bridge_structural_no_score_group_6_empty(before.score, empty);
    }
    nsc_quiet_groups_exclude_score_route(before.score);
}

/// Theorem R, coverage: every successful no-score Message child EOF, of any class, is
/// handled by the no-score route. The selector returns the pushed summary, the score
/// side has neither a source step nor a selection, and the runtime EOF step is the
/// model Complete step: parent acceptance of a score-free return with arena and
/// transcript unchanged.
pub proof fn nsc_every_case_reaches_parent_accept(before: runtime_source::RuntimeFrameImage)
    requires nsc_eof_prestate(before),
    ensures
        runtime_source::runtime_noscore_child_kind(before) == nsc_summary(before),
        bridge::bridge_score_finish_selected(before.score).is_none(),
        forall|after: bridge::ScoreAdmissionBridge|
            !bridge::bridge_score_source_step(before.score, after),
        runtime_source::runtime_message_driver_step(
            before, runtime_source::RuntimeMessageEvent::Eof,
        ) == Some(runtime_source::RuntimeMessageOutcome::ParentAccepted {
            image: runtime_source::runtime_pre_pop_child_accept(before).unwrap(),
        }),
        ({
            let image = runtime_source::runtime_pre_pop_child_accept(before).unwrap();
            &&& image.score.admission.frames.len() + 1 == before.score.admission.frames.len()
            &&& image.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& image.score.admission.arena == before.score.admission.arena
            &&& image.score.transcript == before.score.transcript
            &&& image.score.admission.frames[image.score.admission.frames.len() - 1]
                .waiting_id.is_none()
            &&& image.score.admission.frames[image.score.admission.frames.len() - 1]
                .returns.last().kind == nsc_summary(before).unwrap()
            &&& image.score.admission.frames[image.score.admission.frames.len() - 1]
                .returns.last().score_handle.is_none()
        }),
{
    nsc_route_selects_every_case(before);
    runtime_source::runtime_noscore_child_eof_is_checked(before);
    nsc_noscore_route_excludes_score_route(before);
}

/// Soundness of the selector against the case split: when the selector returns a kind
/// and the model Complete step succeeds, the state is a successful no-score EOF
/// prestate and the selected kind is the summary Complete pushes. On the ESet and EMap
/// arms only an EMap child under tag 23 can be selected, by the selector's EMap arm,
/// which reads the pushed summary itself; elsewhere both summaries agree.
pub proof fn nsc_selected_kind_is_pushed_summary(before: runtime_source::RuntimeFrameImage)
    requires
        runtime_source::runtime_noscore_child_kind(before).is_some(),
        runtime_source::runtime_pre_pop_child_accept(before).is_some(),
    ensures
        nsc_eof_prestate(before),
        nsc_summary(before) == runtime_source::runtime_noscore_child_kind(before),
{
    nsc_complete_step_facts(before);
    let parent = nsc_parent(before);
    let child = nsc_child(before);
    let arena = before.score.admission.arena;
    if nsc_set_map_arm(parent) {
        assert(admission::concrete_derived_summary_with_arena(parent, child, arena)
            == admission::admission_expr_structural_payload_with_arena(parent, child, arena));
        if !(child.family == admission::AdmissionFamily::Other && child.frame_kind == 1
            && (child.program_type_id == 29 || child.program_type_id == 30))
        {
            assert(admission::admission_expr_structural_payload(parent, child).is_none());
        } else if child.program_type_id == 29 {
            assert(admission::concrete_derived_summary(parent, child).is_none());
        } else if parent.waiting_tag == Some(22u32) {
            assert(admission::admission_expr_structural_payload(parent, child).is_none());
        } else {
            assert(admission::concrete_derived_summary(parent, child).is_none());
        }
    } else {
        assert(admission::concrete_derived_summary_with_arena(parent, child, arena)
            == admission::concrete_derived_summary(parent, child));
    }
}

/// The parent-acceptance outcome that Theorem R gives for a witness image.
pub open spec fn nsc_accepts_at_eof(before: runtime_source::RuntimeFrameImage) -> bool {
    runtime_source::runtime_message_driver_step(
        before, runtime_source::RuntimeMessageEvent::Eof,
    ) == Some(runtime_source::RuntimeMessageOutcome::ParentAccepted {
        image: runtime_source::runtime_pre_pop_child_accept(before).unwrap(),
    })
}

/// Routed witness W1 (EZipper, PathMap absent): the runtime module's image of an Expr
/// parent waiting on EZipper (tag 33) and an EZipper child at EOF without its PathMap is a
/// successful no-score Message child EOF of the EZipper class, and the runtime EOF step is
/// the model Complete step.
pub proof fn nsc_ezipper_absent_routed_witness()
    ensures
        nsc_eof_prestate(runtime_source::runtime_noscore_ezipper_absent_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_ezipper_absent_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::EZipperPathMapAbsent),
        nsc_summary(runtime_source::runtime_noscore_ezipper_absent_before())
            == Some(runtime_source::runtime_noscore_absent_expr_payload()),
        runtime_source::runtime_noscore_child_kind(
            runtime_source::runtime_noscore_ezipper_absent_before(),
        ) == Some(runtime_source::runtime_noscore_absent_expr_payload()),
        bridge::bridge_score_finish_selected(
            runtime_source::runtime_noscore_ezipper_absent_before().score,
        ).is_none(),
        forall|after: bridge::ScoreAdmissionBridge|
            !bridge::bridge_score_source_step(
                runtime_source::runtime_noscore_ezipper_absent_before().score, after,
            ),
        nsc_accepts_at_eof(runtime_source::runtime_noscore_ezipper_absent_before()),
{
    let before = runtime_source::runtime_noscore_ezipper_absent_before();
    runtime_source::runtime_noscore_ezipper_absent_modeled_witness();
    assert(runtime_source::runtime_noscore_child_eof_accept(before).is_some());
    nsc_selected_kind_is_pushed_summary(before);
    nsc_every_case_reaches_parent_accept(before);
}

/// Routed witness W2 (EMap, incomplete): the runtime module's image of an Expr parent
/// waiting on EMap (tag 23) and an EMap child at EOF holding one KeyValuePair without key
/// or value is a successful no-score Message child EOF of the EMap class whose arena-free
/// summary is absent. The selector's EMap arm selects it from the pushed summary, so the
/// runtime EOF step is the model Complete step.
pub proof fn nsc_emap_incomplete_routed_witness()
    ensures
        nsc_eof_prestate(runtime_source::runtime_noscore_emap_incomplete_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_emap_incomplete_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::EMapIncomplete),
        nsc_summary(runtime_source::runtime_noscore_emap_incomplete_before())
            == Some(runtime_source::runtime_noscore_absent_expr_payload()),
        admission::concrete_derived_summary(
            nsc_parent(runtime_source::runtime_noscore_emap_incomplete_before()),
            nsc_child(runtime_source::runtime_noscore_emap_incomplete_before()),
        ).is_none(),
        runtime_source::runtime_noscore_child_kind(
            runtime_source::runtime_noscore_emap_incomplete_before(),
        ) == Some(runtime_source::runtime_noscore_absent_expr_payload()),
        bridge::bridge_score_finish_selected(
            runtime_source::runtime_noscore_emap_incomplete_before().score,
        ).is_none(),
        forall|after: bridge::ScoreAdmissionBridge|
            !bridge::bridge_score_source_step(
                runtime_source::runtime_noscore_emap_incomplete_before().score, after,
            ),
        nsc_accepts_at_eof(runtime_source::runtime_noscore_emap_incomplete_before()),
{
    let before = runtime_source::runtime_noscore_emap_incomplete_before();
    runtime_source::runtime_noscore_emap_incomplete_modeled_witness();
    assert(runtime_source::runtime_noscore_child_eof_accept(before).is_some());
    nsc_selected_kind_is_pushed_summary(before);
    nsc_every_case_reaches_parent_accept(before);
}

/// Witness images for classes that have no modeled runtime witness.
pub open spec fn nsc_fixed_point_before() -> runtime_source::RuntimeFrameImage {
    runtime_source::runtime_noscore_leaf_before(
        admission::AdmissionFamily::Expr, 37, admission::AdmissionFamily::FixedPointFrame, 26, 2,
    )
}

pub open spec fn nsc_unit_before() -> runtime_source::RuntimeFrameImage {
    runtime_source::runtime_noscore_leaf_before(
        admission::AdmissionFamily::GUnforgeable, 4, admission::AdmissionFamily::Other, 64, 0,
    )
}

pub open spec fn nsc_var_ref_before() -> runtime_source::RuntimeFrameImage {
    runtime_source::runtime_noscore_leaf_before(
        admission::AdmissionFamily::Connective, 4, admission::AdmissionFamily::Other, 56, 2,
    )
}

pub open spec fn nsc_connective_body_before() -> runtime_source::RuntimeFrameImage {
    runtime_source::runtime_noscore_leaf_before(
        admission::AdmissionFamily::Connective, 1, admission::AdmissionFamily::Other, 57, 1,
    )
}

pub open spec fn nsc_bytes_before() -> runtime_source::RuntimeFrameImage {
    runtime_source::runtime_noscore_leaf_before(
        admission::AdmissionFamily::GUnforgeable, 1, admission::AdmissionFamily::Other, 61, 1,
    )
}

/// Non-vacuity, FixedPoint class (Expr tag 37 -> GFixedPoint).
pub proof fn nsc_inhabited_fixed_point_class()
    ensures
        nsc_eof_prestate(nsc_fixed_point_before()),
        nsc_case_of_program(nsc_child(nsc_fixed_point_before()).program_type_id)
            == Some(NoScoreChildEofCase::FixedPointLeaf),
        nsc_summary(nsc_fixed_point_before()) == Some(
            admission::AdmissionReturnedKind::FixedPoint { unscaled: Seq::empty(), scale: 0 },
        ),
        nsc_accepts_at_eof(nsc_fixed_point_before()),
{
    let before = nsc_fixed_point_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    runtime_source::runtime_noscore_witness_required_slots(parent);
    runtime_source::runtime_noscore_witness_required_slots(child);
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    runtime_source::runtime_noscore_leaf_image_wf(
        admission::AdmissionFamily::Expr, 37, admission::AdmissionFamily::FixedPointFrame, 26, 2,
    );
    let kind = admission::AdmissionReturnedKind::FixedPoint { unscaled: Seq::empty(), scale: 0 };
    assert(admission::admission_fixed_point_fields(child) == Some((Seq::<u8>::empty(), 0u32)));
    assert(admission::concrete_derived_summary_with_arena(parent, child, state.arena)
        == Some(kind));
    reveal(runtime_source::runtime_pre_pop_child_accept);
    assert(admission::admission_complete_child_step(state, 0, 1, kind).is_some());
    assert(runtime_source::runtime_pre_pop_child_accept(before).is_some());
    nsc_every_case_reaches_parent_accept(before);
}

/// Non-vacuity, Unit class (GUnforgeable tag 4 -> GSysAuthToken).
pub proof fn nsc_inhabited_unit_class()
    ensures
        nsc_eof_prestate(nsc_unit_before()),
        nsc_case_of_program(nsc_child(nsc_unit_before()).program_type_id)
            == Some(NoScoreChildEofCase::UnitLeaf),
        nsc_summary(nsc_unit_before()) == Some(admission::AdmissionReturnedKind::Unit),
        nsc_accepts_at_eof(nsc_unit_before()),
{
    let before = nsc_unit_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    runtime_source::runtime_noscore_witness_required_slots(parent);
    runtime_source::runtime_noscore_witness_required_slots(child);
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    runtime_source::runtime_noscore_leaf_image_wf(
        admission::AdmissionFamily::GUnforgeable, 4, admission::AdmissionFamily::Other, 64, 0,
    );
    let kind = admission::AdmissionReturnedKind::Unit;
    assert(admission::concrete_derived_summary_with_arena(parent, child, state.arena)
        == Some(kind));
    reveal(runtime_source::runtime_pre_pop_child_accept);
    assert(admission::admission_complete_child_step(state, 0, 1, kind).is_some());
    assert(runtime_source::runtime_pre_pop_child_accept(before).is_some());
    nsc_every_case_reaches_parent_accept(before);
}

/// Non-vacuity, VarRef class (Connective tag 4 -> VarRef).
pub proof fn nsc_inhabited_var_ref_class()
    ensures
        nsc_eof_prestate(nsc_var_ref_before()),
        nsc_case_of_program(nsc_child(nsc_var_ref_before()).program_type_id)
            == Some(NoScoreChildEofCase::VarRefLeaf),
        nsc_summary(nsc_var_ref_before()) == Some(
            admission::AdmissionReturnedKind::VarRef { index: 0, depth: 0 },
        ),
        nsc_accepts_at_eof(nsc_var_ref_before()),
{
    let before = nsc_var_ref_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    runtime_source::runtime_noscore_witness_required_slots(parent);
    runtime_source::runtime_noscore_witness_required_slots(child);
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    runtime_source::runtime_noscore_leaf_image_wf(
        admission::AdmissionFamily::Connective, 4, admission::AdmissionFamily::Other, 56, 2,
    );
    let kind = admission::AdmissionReturnedKind::VarRef { index: 0, depth: 0 };
    assert(admission::admission_var_ref_fields(child) == Some((0i32, 0i32)));
    assert(admission::concrete_derived_summary_with_arena(parent, child, state.arena)
        == Some(kind));
    reveal(runtime_source::runtime_pre_pop_child_accept);
    assert(admission::admission_complete_child_step(state, 0, 1, kind).is_some());
    assert(runtime_source::runtime_pre_pop_child_accept(before).is_some());
    nsc_every_case_reaches_parent_accept(before);
}

/// Non-vacuity, ConnectiveBody class (Connective tag 1 -> ConnectiveBody).
pub proof fn nsc_inhabited_connective_body_class()
    ensures
        nsc_eof_prestate(nsc_connective_body_before()),
        nsc_case_of_program(nsc_child(nsc_connective_body_before()).program_type_id)
            == Some(NoScoreChildEofCase::ConnectiveBodyLeaf),
        nsc_summary(nsc_connective_body_before()) == Some(
            admission::AdmissionReturnedKind::ConnectiveBody { par_handles: Seq::empty() },
        ),
        nsc_accepts_at_eof(nsc_connective_body_before()),
{
    let before = nsc_connective_body_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    runtime_source::runtime_noscore_witness_required_slots(parent);
    runtime_source::runtime_noscore_witness_required_slots(child);
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    runtime_source::runtime_noscore_leaf_image_wf(
        admission::AdmissionFamily::Connective, 1, admission::AdmissionFamily::Other, 57, 1,
    );
    let kind = admission::AdmissionReturnedKind::ConnectiveBody { par_handles: Seq::empty() };
    assert(admission::admission_connective_body_handles(child) == Some(Seq::<usize>::empty()));
    assert(admission::concrete_derived_summary_with_arena(parent, child, state.arena)
        == Some(kind));
    reveal(runtime_source::runtime_pre_pop_child_accept);
    assert(admission::admission_complete_child_step(state, 0, 1, kind).is_some());
    assert(runtime_source::runtime_pre_pop_child_accept(before).is_some());
    nsc_every_case_reaches_parent_accept(before);
}

/// Non-vacuity, Bytes class (GUnforgeable tag 1 -> GPrivate).
pub proof fn nsc_inhabited_bytes_class()
    ensures
        nsc_eof_prestate(nsc_bytes_before()),
        nsc_case_of_program(nsc_child(nsc_bytes_before()).program_type_id)
            == Some(NoScoreChildEofCase::BytesLeaf),
        nsc_summary(nsc_bytes_before()) == Some(
            admission::AdmissionReturnedKind::Bytes { value: Seq::empty() },
        ),
        nsc_accepts_at_eof(nsc_bytes_before()),
{
    let before = nsc_bytes_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    runtime_source::runtime_noscore_witness_required_slots(parent);
    runtime_source::runtime_noscore_witness_required_slots(child);
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    runtime_source::runtime_noscore_leaf_image_wf(
        admission::AdmissionFamily::GUnforgeable, 1, admission::AdmissionFamily::Other, 61, 1,
    );
    let kind = admission::AdmissionReturnedKind::Bytes { value: Seq::empty() };
    assert(admission::admission_bytes_leaf_payload(child) == Some(Seq::<u8>::empty()));
    assert(admission::concrete_derived_summary_with_arena(parent, child, state.arena)
        == Some(kind));
    reveal(runtime_source::runtime_pre_pop_child_accept);
    assert(admission::admission_complete_child_step(state, 0, 1, kind).is_some());
    assert(runtime_source::runtime_pre_pop_child_accept(before).is_some());
    nsc_every_case_reaches_parent_accept(before);
}

/// Non-vacuity from the modeled runtime witnesses: each one is a successful
/// no-score EOF prestate of its class (selector soundness turns the witness's
/// ParentAccepted EOF step into the prestate).
pub proof fn nsc_inhabited_receive_bind_class()
    ensures
        nsc_eof_prestate(runtime_source::runtime_noscore_receive_bind_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_receive_bind_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::ReceiveBindSourceAbsent),
{
    let before = runtime_source::runtime_noscore_receive_bind_before();
    runtime_source::runtime_noscore_receive_bind_modeled_witness();
    assert(runtime_source::runtime_noscore_child_eof_accept(before).is_some());
    nsc_selected_kind_is_pushed_summary(before);
}

pub proof fn nsc_inhabited_match_case_class()
    ensures
        nsc_eof_prestate(runtime_source::runtime_noscore_match_case_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_match_case_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::MatchCaseRequiredAbsent),
{
    let before = runtime_source::runtime_noscore_match_case_before();
    runtime_source::runtime_noscore_match_case_modeled_witness();
    assert(runtime_source::runtime_noscore_child_eof_accept(before).is_some());
    nsc_selected_kind_is_pushed_summary(before);
}

pub proof fn nsc_inhabited_big_rational_class()
    ensures
        nsc_eof_prestate(runtime_source::runtime_noscore_big_rational_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_big_rational_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::BigRationalLeaf),
{
    let before = runtime_source::runtime_noscore_big_rational_before();
    runtime_source::runtime_noscore_big_rational_modeled_witness();
    assert(runtime_source::runtime_noscore_child_eof_accept(before).is_some());
    nsc_selected_kind_is_pushed_summary(before);
}

pub proof fn nsc_inhabited_evar_class()
    ensures
        nsc_eof_prestate(runtime_source::runtime_noscore_evar_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_evar_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::EVarAbsent),
{
    let before = runtime_source::runtime_noscore_evar_before();
    runtime_source::runtime_noscore_evar_modeled_witness();
    assert(runtime_source::runtime_noscore_child_eof_accept(before).is_some());
    nsc_selected_kind_is_pushed_summary(before);
}

pub proof fn nsc_inhabited_structural_class()
    ensures
        nsc_eof_prestate(runtime_source::runtime_noscore_expr_unary_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_expr_unary_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::ExprStructuralMissingPar),
        nsc_eof_prestate(runtime_source::runtime_noscore_expr_binary_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_expr_binary_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::ExprStructuralMissingPar),
{
    let unary = runtime_source::runtime_noscore_expr_unary_before();
    let binary = runtime_source::runtime_noscore_expr_binary_before();
    runtime_source::runtime_noscore_expr_structural_modeled_witness();
    assert(runtime_source::runtime_noscore_child_eof_accept(unary).is_some());
    assert(runtime_source::runtime_noscore_child_eof_accept(binary).is_some());
    nsc_selected_kind_is_pushed_summary(unary);
    nsc_selected_kind_is_pushed_summary(binary);
}

pub proof fn nsc_inhabited_emethod_class()
    ensures
        nsc_eof_prestate(runtime_source::runtime_noscore_emethod_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_emethod_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::EMethodMissingTarget),
{
    let before = runtime_source::runtime_noscore_emethod_before();
    runtime_source::runtime_noscore_emethod_modeled_witness();
    assert(runtime_source::runtime_noscore_child_eof_accept(before).is_some());
    nsc_selected_kind_is_pushed_summary(before);
}

pub proof fn nsc_inhabited_key_value_pair_class()
    ensures
        nsc_eof_prestate(runtime_source::runtime_noscore_key_value_pair_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_key_value_pair_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::KeyValuePairUnderEMap),
{
    let before = runtime_source::runtime_noscore_key_value_pair_before();
    runtime_source::runtime_noscore_key_value_pair_modeled_witness();
    assert(runtime_source::runtime_noscore_child_eof_accept(before).is_some());
    nsc_selected_kind_is_pushed_summary(before);
}

pub proof fn nsc_inhabited_opaque_class()
    ensures
        nsc_eof_prestate(runtime_source::runtime_noscore_opaque_before()),
        nsc_case_of_program(
            nsc_child(runtime_source::runtime_noscore_opaque_before()).program_type_id,
        ) == Some(NoScoreChildEofCase::DescriptorOpaque),
{
    let before = runtime_source::runtime_noscore_opaque_before();
    runtime_source::runtime_noscore_opaque_modeled_witness();
    assert(runtime_source::runtime_noscore_child_eof_accept(before).is_some());
    nsc_selected_kind_is_pushed_summary(before);
}

/// Child programs of Message edges whose summaries always carry a score.
pub open spec fn nsc_always_scored_program(program: u16) -> bool {
    program == 1 || program == 6 || program == 8 || program == 9 || program == 10
        || program == 11 || program == 12 || program == 13 || program == 15 || program == 19
        || program == 20 || program == 22 || program == 23 || program == 24 || program == 27
        || program == 28 || program == 29 || program == 31 || program == 55 || program == 60
}

/// Descriptor arms (parent program, field tag) through which each class is reached.
pub open spec fn nsc_class_arm(case: NoScoreChildEofCase, parent: u16, tag: u32) -> bool {
    match case {
        NoScoreChildEofCase::ReceiveBindSourceAbsent => parent == 19 && tag == 1,
        NoScoreChildEofCase::MatchCaseRequiredAbsent => parent == 22 && tag == 2,
        NoScoreChildEofCase::BigRationalLeaf => parent == 24 && tag == 36,
        NoScoreChildEofCase::FixedPointLeaf => parent == 24 && tag == 37,
        NoScoreChildEofCase::UnitLeaf => (parent == 6 && tag == 3) || (parent == 60 && tag == 4),
        NoScoreChildEofCase::VarRefLeaf => parent == 55 && tag == 4,
        NoScoreChildEofCase::ConnectiveBodyLeaf => parent == 55 && (tag == 1 || tag == 2),
        NoScoreChildEofCase::BytesLeaf => parent == 60 && 1 <= tag && tag <= 3,
        NoScoreChildEofCase::EVarAbsent => parent == 24 && tag == 19,
        NoScoreChildEofCase::ExprStructuralMissingPar => parent == 24
            && ((5 <= tag && tag <= 18) || (27 <= tag && tag <= 31)),
        NoScoreChildEofCase::EMethodMissingTarget => parent == 24 && tag == 24,
        NoScoreChildEofCase::KeyValuePairUnderEMap => parent == 30 && tag == 1,
        NoScoreChildEofCase::EZipperPathMapAbsent => parent == 24 && tag == 33,
        NoScoreChildEofCase::EMapIncomplete => parent == 24 && tag == 23,
        NoScoreChildEofCase::DescriptorOpaque => (parent == 2 && (tag == 1 || tag == 5))
            || (parent == 18 && tag == 1),
    }
}

/// One descriptor entry is classified: a Message child edge names either an
/// always-scored program or a class of the case split, through that class's arm.
pub open spec fn nsc_descriptor_entry_classified(parent: u16, tag: u32) -> bool {
    match table::generated_descriptor(parent, tag) {
        Some(descriptor) => descriptor.frame_kind != 1 || match descriptor.child_type_id {
            Some(child) => match nsc_case_of_program(child) {
                Some(case) => nsc_class_arm(case, parent, tag),
                None => nsc_always_scored_program(child),
            },
            None => false,
        },
        None => true,
    }
}

pub proof fn nsc_descriptor_low_entries(parent: u16, tag: u32)
    requires parent <= 12,
    ensures nsc_descriptor_entry_classified(parent, tag),
{
}

pub proof fn nsc_descriptor_middle_entries(parent: u16, tag: u32)
    requires 13 <= parent && parent <= 23,
    ensures nsc_descriptor_entry_classified(parent, tag),
{
}

pub proof fn nsc_descriptor_expr_entries(parent: u16, tag: u32)
    requires parent == 24,
    ensures nsc_descriptor_entry_classified(parent, tag),
{
}

pub proof fn nsc_descriptor_upper_entries(parent: u16, tag: u32)
    requires 25 <= parent && parent <= 40,
    ensures nsc_descriptor_entry_classified(parent, tag),
{
}

pub proof fn nsc_descriptor_high_entries(parent: u16, tag: u32)
    requires 41 <= parent,
    ensures nsc_descriptor_entry_classified(parent, tag),
{
}

/// Theorem T1, the case split is exhaustive over the descriptor table: every
/// Message (frame_kind 1) child edge of the generated table names a child
/// program that is always scored or that the case split maps to a class whose
/// arm is exactly this edge.
pub proof fn nsc_descriptor_children_classified(parent: u16, tag: u32)
    ensures nsc_descriptor_entry_classified(parent, tag),
{
    if parent <= 12 {
        nsc_descriptor_low_entries(parent, tag);
    } else if parent <= 23 {
        nsc_descriptor_middle_entries(parent, tag);
    } else if parent == 24 {
        nsc_descriptor_expr_entries(parent, tag);
    } else if parent <= 40 {
        nsc_descriptor_upper_entries(parent, tag);
    } else {
        nsc_descriptor_high_entries(parent, tag);
    }
}

/// Theorem T2, the converse: every class arm is a Message edge of the table
/// whose child program the case split maps to that class.
pub proof fn nsc_class_arms_are_descriptor_edges(
    case: NoScoreChildEofCase, parent: u16, tag: u32,
)
    requires nsc_class_arm(case, parent, tag),
    ensures
        table::generated_descriptor(parent, tag).is_some(),
        table::generated_descriptor(parent, tag).unwrap().frame_kind == 1,
        table::generated_descriptor(parent, tag).unwrap().child_type_id.is_some(),
        nsc_case_of_program(
            table::generated_descriptor(parent, tag).unwrap().child_type_id.unwrap(),
        ) == Some(case),
{
}

/// One descriptor arm of each class, so that no class is empty in the table.
pub open spec fn nsc_class_witness_arm(case: NoScoreChildEofCase) -> (u16, u32) {
    match case {
        NoScoreChildEofCase::ReceiveBindSourceAbsent => (19, 1),
        NoScoreChildEofCase::MatchCaseRequiredAbsent => (22, 2),
        NoScoreChildEofCase::BigRationalLeaf => (24, 36),
        NoScoreChildEofCase::FixedPointLeaf => (24, 37),
        NoScoreChildEofCase::UnitLeaf => (60, 4),
        NoScoreChildEofCase::VarRefLeaf => (55, 4),
        NoScoreChildEofCase::ConnectiveBodyLeaf => (55, 1),
        NoScoreChildEofCase::BytesLeaf => (60, 1),
        NoScoreChildEofCase::EVarAbsent => (24, 19),
        NoScoreChildEofCase::ExprStructuralMissingPar => (24, 5),
        NoScoreChildEofCase::EMethodMissingTarget => (24, 24),
        NoScoreChildEofCase::KeyValuePairUnderEMap => (30, 1),
        NoScoreChildEofCase::EZipperPathMapAbsent => (24, 33),
        NoScoreChildEofCase::EMapIncomplete => (24, 23),
        NoScoreChildEofCase::DescriptorOpaque => (2, 1),
    }
}

pub proof fn nsc_every_class_has_descriptor_arm(case: NoScoreChildEofCase)
    ensures
        nsc_class_arm(case, nsc_class_witness_arm(case).0, nsc_class_witness_arm(case).1),
        table::generated_descriptor(
            nsc_class_witness_arm(case).0, nsc_class_witness_arm(case).1,
        ).is_some(),
{
    nsc_class_arms_are_descriptor_edges(
        case, nsc_class_witness_arm(case).0, nsc_class_witness_arm(case).1,
    );
}

/// The always-scored programs and the case split's domain are disjoint.
pub proof fn nsc_always_scored_has_no_case(program: u16)
    requires nsc_always_scored_program(program),
    ensures nsc_case_of_program(program).is_none(),
{
}

/// Corollary of Theorem C: no successful no-score Message child EOF has an
/// always-scored child program.
pub proof fn nsc_always_scored_child_has_no_prestate(before: runtime_source::RuntimeFrameImage)
    requires nsc_eof_prestate(before),
    ensures !nsc_always_scored_program(nsc_child(before).program_type_id),
{
    nsc_case_split_total(before);
    if nsc_always_scored_program(nsc_child(before).program_type_id) {
        nsc_always_scored_has_no_case(nsc_child(before).program_type_id);
    }
}

/// Coverage theorem (headline), universal form: every successful no-score Message
/// child EOF has a class, and for every class the runtime EOF step is the model Complete
/// step (parent acceptance of the pushed summary). The selector returns the pushed
/// summary, the score-finish selector proposes nothing, and no score source step exists.
pub proof fn nsc_eof_coverage_universal(before: runtime_source::RuntimeFrameImage)
    requires nsc_eof_prestate(before),
    ensures
        nsc_case_of_program(nsc_child(before).program_type_id).is_some(),
        nsc_accepts_at_eof(before),
        runtime_source::runtime_noscore_child_kind(before) == nsc_summary(before),
        bridge::bridge_score_finish_selected(before.score).is_none(),
        forall|after: bridge::ScoreAdmissionBridge|
            !bridge::bridge_score_source_step(before.score, after),
{
    nsc_case_split_total(before);
    nsc_every_case_reaches_parent_accept(before);
}

// Disabled: false, the source route accepts an incomplete EMap; see the same-name lemma below.
// pub proof fn nsc_source_noscore_route_on_emap_ezipper(
//     before: runtime_source::RuntimeFrameImage, result: runtime_source::SourceMessageFinishResult,
// )
//     requires
//         nsc_eof_prestate(before),
//         nsc_child(before).program_type_id == 30 || nsc_child(before).program_type_id == 32,
//     ensures
//         nsc_child(before).program_type_id == 30
//             ==> runtime_source::source_noscore_message_eof_child_accept(before, result).is_none(),
//         nsc_child(before).program_type_id == 32
//             ==> runtime_source::source_noscore_message_eof_child_accept(
//                 before, runtime_source::source_unchanged_finish_result(before),
//             ) == runtime_source::runtime_pre_pop_child_accept(before),
// {
//     nsc_complete_step_facts(before);
//     nsc_case_split_total(before);
//     reveal(runtime_source::source_noscore_message_finish_ready);
//     let parent = nsc_parent(before);
//     let child = nsc_child(before);
//     let arena = before.score.admission.arena;
//     let kind = nsc_summary(before).unwrap();
//     if child.program_type_id == 30 {
//         assert(admission::concrete_derived_summary(parent, child).is_none());
//     } else {
//         let state = before.score.admission;
//         assert(state.frames.update(state.frames.len() - 1, state.frames.last())
//             =~= state.frames);
//         assert(runtime_source::source_message_finish_bridge(
//             before, runtime_source::source_unchanged_finish_result(before),
//         ) == Some(before.score));
//         nsc_classify_scored_prefix(parent, child, arena, kind);
//         assert(admission::concrete_derived_summary(parent, child) == Some(kind));
//     }
// }

/// The source-level no-score route agrees with the runtime route on the EMap (30) and
/// EZipper (32) classes, the two classes the runtime selector reaches through its EMap
/// and EZipper arms: the unchanged finish result keeps the bridge, and for every finish
/// result that keeps it, `source_noscore_message_eof_child_accept` accepts with the
/// model Complete image, which is the image the runtime EOF step accepts.
pub proof fn nsc_source_noscore_route_on_emap_ezipper(
    before: runtime_source::RuntimeFrameImage, result: runtime_source::SourceMessageFinishResult,
)
    requires
        nsc_eof_prestate(before),
        nsc_child(before).program_type_id == 30 || nsc_child(before).program_type_id == 32,
    ensures
        runtime_source::source_message_finish_bridge(
            before, runtime_source::source_unchanged_finish_result(before),
        ) == Some(before.score),
        runtime_source::source_message_finish_bridge(before, result) == Some(before.score)
            ==> runtime_source::source_noscore_message_eof_child_accept(before, result)
                == runtime_source::runtime_pre_pop_child_accept(before),
        nsc_accepts_at_eof(before),
{
    nsc_every_case_reaches_parent_accept(before);
    let state = before.score.admission;
    assert(state.frames.update(state.frames.len() - 1, state.frames.last()) =~= state.frames);
    if runtime_source::source_message_finish_bridge(before, result) == Some(before.score) {
        runtime_source::source_noscore_route_agrees_with_runtime(before, result);
    }
}

}
