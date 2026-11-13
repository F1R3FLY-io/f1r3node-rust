use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::any;
use proptest::strategy::{BoxedStrategy, Strategy};
use proptest::test_runner::{
    Config as ProptestConfig, RngSeed, TestCaseError, TestError, TestRunner,
};

use super::*;
use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::cost_signature_atom::Form;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::tagged_continuation::TaggedCont;
use crate::rhoapi::var::{VarInstance, WildcardMsg};
use crate::rhoapi::{
    Bundle, Connective, ConnectiveBody, CostBoundSignature, CostRegion, CostSignature,
    CostSignatureAtom, CostTokenStack, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap,
    EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPercentPercent,
    EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, Expr, GBigRational, GDeployId, GDeployerId,
    GFixedPoint, GPrivate, GSysAuthToken, GUnforgeable, If, KeyValuePair, Match, MatchCase, New,
    Par, ParWithRandom, Receive, ReceiveBind, Send, TaggedContinuation, Var, VarRef,
};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::protobuf_encoder;
use crate::rust::rholang::protobuf_schema::ProtobufNode;

const FINISH_INVARIANT_BINDINGS: &[(&str, &str)] = &[
    (
        "finish_handle_facts",
        "scored_finish_handle_has_the_handle_facts",
    ),
    (
        "finish_top_handle",
        "selected_finish_leaves_a_top_handle_with_its_facts",
    ),
    (
        "finish_child_return_facts",
        "child_pop_return_has_the_child_return_facts",
    ),
    (
        "finish_root_return_facts",
        "scored_root_return_has_the_root_return_facts",
    ),
    (
        "handle_facts_from_agreement",
        "score_body_agreement_gives_the_handle_facts",
    ),
    (
        "top_handle_from_agreement",
        "top_frame_agreement_gives_the_top_handle",
    ),
    (
        "attach_agreement_holds",
        "attached_finish_handle_agrees_with_its_frame",
    ),
    (
        "attach_agreement_everywhere",
        "every_attach_of_a_nested_run_agrees_with_its_frame",
    ),
    (
        "handle_after_raw_par",
        "raw_par_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_cost_atom_absent",
        "cost_atom_absent_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_cost_atom_bytes",
        "cost_atom_bytes_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_cost_bound",
        "cost_bound_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_cost_atom_bound_forward",
        "cost_atom_bound_forward_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_cost_signature",
        "cost_signature_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_cost_region",
        "cost_region_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_cost_token_stack",
        "cost_token_stack_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_bundle",
        "bundle_finish_has_the_top_handle_facts",
    ),
    ("handle_after_if", "if_finish_has_the_top_handle_facts"),
    ("handle_after_send", "send_finish_has_the_top_handle_facts"),
    (
        "handle_after_match_component",
        "match_component_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_absent_specialized",
        "absent_specialized_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_absent_connective",
        "absent_connective_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_receive_bind",
        "receive_bind_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_match_case",
        "match_case_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_bool_oneof",
        "bool_oneof_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_expr_atom",
        "expr_atom_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_expr_numeric",
        "expr_numeric_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_expr_big_rational",
        "expr_big_rational_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_expr_fixed_point",
        "expr_fixed_point_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_sys_auth_token",
        "sys_auth_token_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_new_empty_collections",
        "new_empty_collections_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_new_one_injection",
        "new_one_injection_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_new_one_absent_injection",
        "new_one_absent_injection_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_new_one_uri",
        "new_one_uri_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_new_collections",
        "new_collections_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_expr_payload_forward",
        "expr_payload_forward_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_expr_structural",
        "expr_structural_finish_has_the_top_handle_facts",
    ),
    ("handle_after_evar", "evar_finish_has_the_top_handle_facts"),
    (
        "handle_after_ordered_sequence",
        "ordered_sequence_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_emethod",
        "emethod_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_emap_finish",
        "emap_finish_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_connective_not",
        "connective_not_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_connective_varref",
        "connective_varref_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_connective_collection",
        "connective_collection_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_unforgeable_bytes",
        "unforgeable_bytes_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_receive_present_condition",
        "receive_present_condition_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_receive_absent_condition",
        "receive_absent_condition_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_nonsemantic",
        "nonsemantic_finish_has_the_top_handle_facts",
    ),
    ("handle_after_var", "var_finish_has_the_top_handle_facts"),
    (
        "handle_after_epathmap_owner",
        "epathmap_owner_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_ezipper",
        "ezipper_finish_has_the_top_handle_facts",
    ),
    (
        "handle_routed_group_1",
        "finish_group_1_routes_have_the_top_handle_facts",
    ),
    (
        "handle_routed_group_2",
        "finish_group_2_routes_have_the_top_handle_facts",
    ),
    (
        "handle_routed_group_3",
        "finish_group_3_routes_have_the_top_handle_facts",
    ),
    (
        "handle_routed_group_4",
        "finish_group_4_routes_have_the_top_handle_facts",
    ),
    (
        "handle_routed_group_5",
        "finish_group_5_routes_have_the_top_handle_facts",
    ),
    (
        "handle_routed_group_6",
        "finish_group_6_routes_have_the_top_handle_facts",
    ),
    (
        "handle_after_selected_finish",
        "every_selected_finish_route_has_the_top_handle_facts",
    ),
    (
        "handle_after_epm_source_finish",
        "epathmap_source_finish_has_the_top_handle_facts",
    ),
    (
        "handle_after_product_epm_finish",
        "epathmap_product_finish_has_the_top_handle_facts",
    ),
    (
        "child_return_carries_handle",
        "child_step_return_carries_the_child_handle",
    ),
    (
        "pre_pop_return_carries_handle",
        "pre_pop_child_accept_carries_the_child_handle",
    ),
    (
        "eof_score_child_carries_handle",
        "scored_child_eof_accept_carries_the_child_handle",
    ),
    (
        "root_return_from_top_handle",
        "root_return_follows_from_the_top_handle",
    ),
    (
        "runtime_eof_finish_facts",
        "message_eof_outcomes_have_the_finish_facts",
    ),
    (
        "runtime_epathmap_eof_finish_facts",
        "epathmap_eof_outcomes_have_the_finish_facts",
    ),
    (
        "live_root_score_has_top_handle",
        "live_root_score_step_has_the_top_handle",
    ),
    (
        "live_child_score_has_top_handle",
        "live_child_score_step_has_the_top_handle",
    ),
    (
        "live_root_complete_carries_handle",
        "live_root_complete_carries_the_handle_to_the_return",
    ),
    (
        "handle_owner_witness",
        "nested_epathmap_owner_reaches_the_witness_finish",
    ),
];

const FINISH_INVARIANT_SOURCE: &str =
    include_str!("../../../formal/verus/rpb3_finish_handle_transcript.rs");
const FINISH_PROPERTY_SOURCE: &str = include_str!("rpb3_finish_invariant_properties.rs");
const FINISH_PROPERTY_ENTRY: &str = "check_finish_invariant";
const KIND_INVARIANT_PREFIX: &str = "handle_after_";
const GROUP_INVARIANT_PREFIX: &str = "handle_routed_group_";
const VIOLATION_MARK: &str = "violated: ";
const FINISH_CLASS_COUNT: u8 = 6;
const EPATHMAP_OWNER_TYPE_ID: u16 = 31;
const EPATHMAP_OWNER_APPENDS: usize = 4;
const REPORTED_ROW_FAILURES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Observed {
    Faithful,
    Forged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum FinishKind {
    RawPar = 1,
    CostAtomAbsent,
    CostAtomBytes,
    CostBound,
    CostAtomBoundForward,
    CostSignature,
    CostRegion,
    CostTokenStack,
    Bundle,
    If,
    Send,
    MatchComponent,
    AbsentSpecialized,
    AbsentConnective,
    ReceiveBind,
    MatchCase,
    BoolOneof,
    ExprAtom,
    ExprNumeric,
    ExprBigRational,
    ExprFixedPoint,
    SysAuthToken,
    NewEmptyCollections,
    NewOneInjection,
    NewOneAbsentInjection,
    NewOneUri,
    NewCollections,
    ExprPayloadForward,
    ExprStructural,
    Evar,
    OrderedSequence,
    Emethod,
    EmapFinish,
    ConnectiveNot,
    ConnectiveVarref,
    ConnectiveCollection,
    UnforgeableBytes,
    ReceivePresentCondition,
    ReceiveAbsentCondition,
    Nonsemantic,
    Var,
    EpathmapOwner,
    Ezipper,
}

const FINISH_KINDS: [(FinishKind, &str); 43] = [
    (FinishKind::RawPar, "raw_par"),
    (FinishKind::CostAtomAbsent, "cost_atom_absent"),
    (FinishKind::CostAtomBytes, "cost_atom_bytes"),
    (FinishKind::CostBound, "cost_bound"),
    (FinishKind::CostAtomBoundForward, "cost_atom_bound_forward"),
    (FinishKind::CostSignature, "cost_signature"),
    (FinishKind::CostRegion, "cost_region"),
    (FinishKind::CostTokenStack, "cost_token_stack"),
    (FinishKind::Bundle, "bundle"),
    (FinishKind::If, "if"),
    (FinishKind::Send, "send"),
    (FinishKind::MatchComponent, "match_component"),
    (FinishKind::AbsentSpecialized, "absent_specialized"),
    (FinishKind::AbsentConnective, "absent_connective"),
    (FinishKind::ReceiveBind, "receive_bind"),
    (FinishKind::MatchCase, "match_case"),
    (FinishKind::BoolOneof, "bool_oneof"),
    (FinishKind::ExprAtom, "expr_atom"),
    (FinishKind::ExprNumeric, "expr_numeric"),
    (FinishKind::ExprBigRational, "expr_big_rational"),
    (FinishKind::ExprFixedPoint, "expr_fixed_point"),
    (FinishKind::SysAuthToken, "sys_auth_token"),
    (FinishKind::NewEmptyCollections, "new_empty_collections"),
    (FinishKind::NewOneInjection, "new_one_injection"),
    (
        FinishKind::NewOneAbsentInjection,
        "new_one_absent_injection",
    ),
    (FinishKind::NewOneUri, "new_one_uri"),
    (FinishKind::NewCollections, "new_collections"),
    (FinishKind::ExprPayloadForward, "expr_payload_forward"),
    (FinishKind::ExprStructural, "expr_structural"),
    (FinishKind::Evar, "evar"),
    (FinishKind::OrderedSequence, "ordered_sequence"),
    (FinishKind::Emethod, "emethod"),
    (FinishKind::EmapFinish, "emap_finish"),
    (FinishKind::ConnectiveNot, "connective_not"),
    (FinishKind::ConnectiveVarref, "connective_varref"),
    (FinishKind::ConnectiveCollection, "connective_collection"),
    (FinishKind::UnforgeableBytes, "unforgeable_bytes"),
    (
        FinishKind::ReceivePresentCondition,
        "receive_present_condition",
    ),
    (
        FinishKind::ReceiveAbsentCondition,
        "receive_absent_condition",
    ),
    (FinishKind::Nonsemantic, "nonsemantic"),
    (FinishKind::Var, "var"),
    (FinishKind::EpathmapOwner, "epathmap_owner"),
    (FinishKind::Ezipper, "ezipper"),
];

fn finish_kind_route(kind: FinishKind) -> u8 { kind as u8 }

fn finish_kind_name(kind: FinishKind) -> &'static str {
    FINISH_KINDS[usize::from(finish_kind_route(kind)) - 1].1
}

fn finish_kind_class(kind: FinishKind) -> u8 {
    match finish_kind_route(kind) {
        1..=8 => 1,
        9..=16 => 2,
        17..=24 => 3,
        25..=33 => 4,
        34..=40 => 5,
        _ => 6,
    }
}

fn finish_kind_named(name: &str) -> Option<FinishKind> {
    FINISH_KINDS
        .iter()
        .find(|(_, candidate)| *candidate == name)
        .map(|(kind, _)| *kind)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelFamily {
    Send,
    Receive,
    New,
    Match,
    Bundle,
    If,
    CostTokenStack,
    Expr,
    Connective,
    GUnforgeable,
    ReceiveBindFrame,
    MatchCaseFrame,
    CostSignatureFrame,
    CostRegionFrame,
    BigRationalFrame,
    FixedPointFrame,
    EVarFrame,
    Other,
}

fn model_family(program_type_id: u16) -> ModelFamily {
    match program_type_id {
        8 => ModelFamily::Bundle,
        9 => ModelFamily::CostSignatureFrame,
        12 => ModelFamily::CostRegionFrame,
        13 => ModelFamily::CostTokenStack,
        15 => ModelFamily::Send,
        16 => ModelFamily::ReceiveBindFrame,
        19 => ModelFamily::Receive,
        20 => ModelFamily::New,
        21 => ModelFamily::MatchCaseFrame,
        22 => ModelFamily::Match,
        23 => ModelFamily::If,
        24 => ModelFamily::Expr,
        25 => ModelFamily::BigRationalFrame,
        26 => ModelFamily::FixedPointFrame,
        35 => ModelFamily::EVarFrame,
        55 => ModelFamily::Connective,
        60 => ModelFamily::GUnforgeable,
        _ => ModelFamily::Other,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReturnKind {
    Par,
    Component,
    ReceiveBind {
        source_present: bool,
    },
    MatchCase {
        required_present: bool,
    },
    ExprPayload {
        score_present: bool,
        canonical_fixed_point: bool,
    },
    BigRational,
    FixedPoint,
    CostScore,
    VarScore,
    ConnectiveBody,
    VarRef,
    Bytes,
    Unit,
    KeyValuePair {
        complete: bool,
    },
    Map {
        value_present: bool,
    },
    EPathMapSnapshot,
    Other,
}

fn return_kind(returned: &RootReturnedState) -> ReturnKind {
    match returned {
        RootReturnedState::Opaque => ReturnKind::Other,
        RootReturnedState::Par(_) => ReturnKind::Par,
        RootReturnedState::Component(..) => ReturnKind::Component,
        RootReturnedState::Var(_) => ReturnKind::VarScore,
        RootReturnedState::ReceiveBind { source_present, .. } => ReturnKind::ReceiveBind {
            source_present: *source_present,
        },
        RootReturnedState::CostScore(_) => ReturnKind::CostScore,
        RootReturnedState::MatchCase {
            required_present, ..
        } => ReturnKind::MatchCase {
            required_present: *required_present,
        },
        RootReturnedState::ExprPayload {
            score,
            canonical_fixed_point,
            ..
        } => ReturnKind::ExprPayload {
            score_present: score.is_some(),
            canonical_fixed_point: *canonical_fixed_point,
        },
        RootReturnedState::KeyValuePair { key, value } => ReturnKind::KeyValuePair {
            complete: key.is_some() && value.is_some(),
        },
        RootReturnedState::BigRational { .. } => ReturnKind::BigRational,
        RootReturnedState::FixedPoint { .. } => ReturnKind::FixedPoint,
        RootReturnedState::ConnectiveBody(_) => ReturnKind::ConnectiveBody,
        RootReturnedState::VarRef { .. } => ReturnKind::VarRef,
        RootReturnedState::Bytes(_) => ReturnKind::Bytes,
        RootReturnedState::Unit => ReturnKind::Unit,
    }
}

fn returned_handle(returned: &RootReturnedState) -> Option<CanonicalScoreHandle> {
    match returned {
        RootReturnedState::Par(par) => Some(par.score_handle),
        RootReturnedState::Component(_, handle)
        | RootReturnedState::Var(handle)
        | RootReturnedState::CostScore(handle) => Some(*handle),
        RootReturnedState::ReceiveBind { score, .. }
        | RootReturnedState::MatchCase { score, .. }
        | RootReturnedState::ExprPayload { score, .. } => *score,
        _ => None,
    }
}

fn return_kind_has_score(kind: ReturnKind) -> bool {
    match kind {
        ReturnKind::Par | ReturnKind::Component | ReturnKind::CostScore | ReturnKind::VarScore => {
            true
        }
        ReturnKind::ReceiveBind { source_present } => source_present,
        ReturnKind::MatchCase { required_present } => required_present,
        ReturnKind::ExprPayload { score_present, .. } => score_present,
        _ => false,
    }
}

fn reported_semantic_flag(returned: &RootReturnedState) -> Option<bool> {
    match returned {
        RootReturnedState::Component(component, _) => Some(component.complete),
        RootReturnedState::ReceiveBind { source_present, .. } => Some(*source_present),
        RootReturnedState::MatchCase {
            required_present, ..
        } => Some(*required_present),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
struct FrameReturn {
    slot: usize,
    tag: u32,
    kind: ReturnKind,
    handle: Option<CanonicalScoreHandle>,
}

#[derive(Clone, Debug)]
struct FrameView {
    program_type_id: u16,
    family: ModelFamily,
    seen_slots: Vec<usize>,
    scalars: Vec<(usize, u32)>,
    returns: Vec<FrameReturn>,
}

fn occurrence_tag(
    program: &'static RhoMatchProgram,
    slot: usize,
    variant: Option<&'static str>,
) -> u32 {
    let field = &program.fields[slot];
    match (field.kind, variant) {
        (RhoMatchFieldKind::Oneof(variants), Some(name)) => variants
            .iter()
            .find(|candidate| candidate.name == name)
            .map_or(field.tag, |candidate| candidate.tag),
        _ => field.tag,
    }
}

fn frame_view(frame: &RootFrameState) -> FrameView {
    let occurrence_count = frame.semantic_slots.iter().map(Vec::len).sum();
    let mut scalars = Vec::with_capacity(occurrence_count);
    let mut returns = Vec::with_capacity(occurrence_count);
    for (slot, occurrences) in frame.semantic_slots.iter().enumerate() {
        for occurrence in occurrences {
            let tag = occurrence_tag(frame.program, slot, occurrence.variant);
            match &occurrence.value {
                RootOccurrenceValue::Scalar(_) => scalars.push((slot, tag)),
                RootOccurrenceValue::Message(returned) => returns.push(FrameReturn {
                    slot,
                    tag,
                    kind: return_kind(returned),
                    handle: returned_handle(returned),
                }),
                RootOccurrenceValue::Map { value, .. } => returns.push(FrameReturn {
                    slot,
                    tag,
                    kind: ReturnKind::Map {
                        value_present: value.is_some(),
                    },
                    handle: None,
                }),
                RootOccurrenceValue::EPathMap { .. } => returns.push(FrameReturn {
                    slot,
                    tag,
                    kind: ReturnKind::EPathMapSnapshot,
                    handle: None,
                }),
            }
        }
    }
    FrameView {
        program_type_id: frame.program_type_id,
        family: model_family(frame.program_type_id),
        seen_slots: frame.seen_slots.clone(),
        scalars,
        returns,
    }
}

impl FrameView {
    fn seen(&self, slot: usize) -> bool { self.seen_slots.contains(&slot) }

    fn scalar_slot(&self, slot: usize) -> bool {
        self.scalars
            .iter()
            .any(|(scalar_slot, _)| *scalar_slot == slot)
    }

    fn scalar_count_at(&self, slot: usize) -> usize {
        self.scalars
            .iter()
            .filter(|(scalar_slot, _)| *scalar_slot == slot)
            .count()
    }

    fn par_count_at(&self, slot: usize) -> usize {
        self.returns
            .iter()
            .filter(|returned| returned.slot == slot && returned.kind == ReturnKind::Par)
            .count()
    }

    fn returned_par(&self, slot: usize) -> bool { self.par_count_at(slot) > 0 }

    fn every_return_at(&self, slot: usize, accepts: impl Fn(ReturnKind) -> bool) -> bool {
        self.returns
            .iter()
            .filter(|returned| returned.slot == slot)
            .all(|returned| accepts(returned.kind))
    }

    fn any_return_at(&self, slot: usize, accepts: impl Fn(ReturnKind) -> bool) -> bool {
        self.returns
            .iter()
            .any(|returned| returned.slot == slot && accepts(returned.kind))
    }

    fn family_complete(&self) -> bool {
        match self.family {
            ModelFamily::Send | ModelFamily::Bundle => self.returned_par(0),
            ModelFamily::Receive => {
                self.returned_par(1)
                    && self.every_return_at(0, |kind| {
                        kind == ReturnKind::ReceiveBind {
                            source_present: true,
                        }
                    })
            }
            ModelFamily::New | ModelFamily::ReceiveBindFrame => self.returned_par(1),
            ModelFamily::Match => {
                self.returned_par(0)
                    && self.every_return_at(1, |kind| {
                        kind == ReturnKind::MatchCase {
                            required_present: true,
                        }
                    })
            }
            ModelFamily::If => self.returned_par(0) && self.returned_par(1) && self.returned_par(2),
            ModelFamily::Expr => {
                !self.seen(0)
                    || self.scalar_slot(0)
                    || self.any_return_at(0, |kind| {
                        matches!(
                            kind,
                            ReturnKind::BigRational
                                | ReturnKind::FixedPoint
                                | ReturnKind::ExprPayload {
                                    score_present: true,
                                    ..
                                }
                        )
                    })
            }
            ModelFamily::MatchCaseFrame => self.returned_par(0) && self.returned_par(1),
            ModelFamily::EVarFrame => self.any_return_at(0, |kind| kind == ReturnKind::VarScore),
            ModelFamily::CostTokenStack
            | ModelFamily::Connective
            | ModelFamily::GUnforgeable
            | ModelFamily::CostSignatureFrame
            | ModelFamily::CostRegionFrame
            | ModelFamily::BigRationalFrame
            | ModelFamily::FixedPointFrame
            | ModelFamily::Other => true,
        }
    }

    fn finish_kind(&self) -> Option<FinishKind> {
        let complete = self.family_complete();
        let when_complete = |kind: FinishKind| {
            if complete {
                Some(kind)
            } else {
                Some(FinishKind::Nonsemantic)
            }
        };
        match self.family {
            ModelFamily::Other => self.other_finish_kind(),
            ModelFamily::CostSignatureFrame => Some(FinishKind::CostSignature),
            ModelFamily::CostRegionFrame => Some(FinishKind::CostRegion),
            ModelFamily::CostTokenStack => Some(FinishKind::CostTokenStack),
            ModelFamily::Bundle => when_complete(FinishKind::Bundle),
            ModelFamily::If => when_complete(FinishKind::If),
            ModelFamily::Send => when_complete(FinishKind::Send),
            ModelFamily::Match => when_complete(FinishKind::MatchComponent),
            ModelFamily::ReceiveBindFrame => complete.then_some(FinishKind::ReceiveBind),
            ModelFamily::MatchCaseFrame => complete.then_some(FinishKind::MatchCase),
            ModelFamily::EVarFrame => complete.then_some(FinishKind::Evar),
            ModelFamily::BigRationalFrame | ModelFamily::FixedPointFrame => None,
            ModelFamily::Expr => self.expr_finish_kind(complete),
            ModelFamily::GUnforgeable => self.unforgeable_finish_kind(),
            ModelFamily::Connective => self.connective_finish_kind(),
            ModelFamily::New => self.new_finish_kind(complete),
            ModelFamily::Receive => {
                if !complete {
                    Some(FinishKind::Nonsemantic)
                } else if self.seen(7) {
                    Some(FinishKind::ReceivePresentCondition)
                } else {
                    Some(FinishKind::ReceiveAbsentCondition)
                }
            }
        }
    }

    fn other_finish_kind(&self) -> Option<FinishKind> {
        match self.program_type_id {
            1 => Some(FinishKind::RawPar),
            10 => Some(if !self.seen(0) {
                FinishKind::CostAtomAbsent
            } else if self.scalar_slot(0) {
                FinishKind::CostAtomBytes
            } else {
                FinishKind::CostAtomBoundForward
            }),
            11 => Some(FinishKind::CostBound),
            36..=54 => Some(FinishKind::ExprStructural),
            27..=29 => Some(FinishKind::OrderedSequence),
            33 => Some(FinishKind::Emethod),
            30 => Some(FinishKind::EmapFinish),
            6 => Some(FinishKind::Var),
            31 => Some(FinishKind::EpathmapOwner),
            32 => Some(FinishKind::Ezipper),
            _ => None,
        }
    }

    fn expr_finish_kind(&self, complete: bool) -> Option<FinishKind> {
        if !complete {
            return Some(FinishKind::Nonsemantic);
        }
        if !self.seen(0) {
            return Some(FinishKind::AbsentSpecialized);
        }
        match (self.scalars.as_slice(), self.returns.as_slice()) {
            ([(_, tag)], _) => match tag {
                1 => Some(FinishKind::BoolOneof),
                3 | 4 | 25 | 35 => Some(FinishKind::ExprAtom),
                2 | 34 => Some(FinishKind::ExprNumeric),
                _ => None,
            },
            (_, [returned]) => match returned.kind {
                ReturnKind::BigRational => Some(FinishKind::ExprBigRational),
                ReturnKind::FixedPoint => Some(FinishKind::ExprFixedPoint),
                ReturnKind::ExprPayload { .. } => Some(FinishKind::ExprPayloadForward),
                _ => None,
            },
            _ => None,
        }
    }

    fn unforgeable_finish_kind(&self) -> Option<FinishKind> {
        if !self.seen(0) {
            return Some(FinishKind::AbsentSpecialized);
        }
        match self.returns.as_slice() {
            [returned] if returned.tag == 4 => Some(FinishKind::SysAuthToken),
            _ => Some(FinishKind::UnforgeableBytes),
        }
    }

    fn connective_finish_kind(&self) -> Option<FinishKind> {
        if !self.seen(0) {
            return Some(FinishKind::AbsentConnective);
        }
        match (self.scalars.as_slice(), self.returns.as_slice()) {
            ([_], _) => Some(FinishKind::BoolOneof),
            (_, [returned]) => Some(match returned.tag {
                3 => FinishKind::ConnectiveNot,
                4 => FinishKind::ConnectiveVarref,
                _ => FinishKind::ConnectiveCollection,
            }),
            _ => None,
        }
    }

    fn new_finish_kind(&self, complete: bool) -> Option<FinishKind> {
        if !complete {
            return Some(FinishKind::Nonsemantic);
        }
        if self.seen(2) {
            return Some(if self.scalar_count_at(2) == 1 && self.returns.len() == 1 {
                FinishKind::NewOneUri
            } else {
                FinishKind::NewCollections
            });
        }
        Some(match self.returns.as_slice() {
            [_] => FinishKind::NewEmptyCollections,
            [_, injection] => match injection.kind {
                ReturnKind::Map {
                    value_present: true,
                } => FinishKind::NewOneInjection,
                _ => FinishKind::NewOneAbsentInjection,
            },
            _ => FinishKind::NewCollections,
        })
    }

    fn model_selects(&self, kind: Option<FinishKind>) -> bool {
        match kind {
            None => false,
            Some(FinishKind::ExprStructural) => {
                let operands = if matches!(self.program_type_id, 36 | 37) {
                    1
                } else {
                    2
                };
                self.scalars.is_empty()
                    && self.returns.len() == operands
                    && (0..operands).all(|slot| self.par_count_at(slot) == 1)
            }
            Some(FinishKind::Emethod) => self.par_count_at(1) == 1,
            Some(FinishKind::EmapFinish) => self.every_return_at(0, |kind| {
                kind == ReturnKind::KeyValuePair { complete: true }
            }),
            Some(FinishKind::Ezipper) => {
                self.seen(0)
                    && matches!(self.returns.as_slice(), [returned] if returned.slot == 0
                    && returned.kind
                        == ReturnKind::ExprPayload {
                            score_present: true,
                            canonical_fixed_point: true,
                        })
            }
            Some(_) => true,
        }
    }
}

fn selected_reducer_matches(kind: FinishKind, selected: SemanticKind) -> bool {
    match kind {
        FinishKind::RawPar => selected == SemanticKind::RawPar,
        FinishKind::CostAtomAbsent
        | FinishKind::CostAtomBytes
        | FinishKind::CostAtomBoundForward => selected == SemanticKind::CostSignatureAtom,
        FinishKind::CostBound => selected == SemanticKind::CostBoundSignature,
        FinishKind::CostSignature => selected == SemanticKind::CostSignature,
        FinishKind::CostRegion => selected == SemanticKind::CostRegion,
        FinishKind::CostTokenStack => {
            selected == SemanticKind::Component(ParComponentTag::CostTokenStack)
        }
        FinishKind::Bundle => selected == SemanticKind::Component(ParComponentTag::Bundle),
        FinishKind::If => selected == SemanticKind::Component(ParComponentTag::If),
        FinishKind::Send => selected == SemanticKind::Component(ParComponentTag::Send),
        FinishKind::MatchComponent => selected == SemanticKind::Component(ParComponentTag::Match),
        FinishKind::AbsentSpecialized => {
            matches!(selected, SemanticKind::Expr | SemanticKind::GUnforgeable)
        }
        FinishKind::AbsentConnective
        | FinishKind::ConnectiveNot
        | FinishKind::ConnectiveVarref
        | FinishKind::ConnectiveCollection => selected == SemanticKind::Connective,
        FinishKind::ReceiveBind => selected == SemanticKind::ReceiveBind,
        FinishKind::MatchCase => selected == SemanticKind::MatchCase,
        FinishKind::BoolOneof => matches!(selected, SemanticKind::Expr | SemanticKind::Connective),
        FinishKind::ExprAtom
        | FinishKind::ExprNumeric
        | FinishKind::ExprBigRational
        | FinishKind::ExprFixedPoint
        | FinishKind::ExprPayloadForward => selected == SemanticKind::Expr,
        FinishKind::SysAuthToken | FinishKind::UnforgeableBytes => {
            selected == SemanticKind::GUnforgeable
        }
        FinishKind::NewEmptyCollections
        | FinishKind::NewOneInjection
        | FinishKind::NewOneAbsentInjection
        | FinishKind::NewOneUri
        | FinishKind::NewCollections => selected == SemanticKind::Component(ParComponentTag::New),
        FinishKind::ExprStructural => {
            matches!(
                selected,
                SemanticKind::ExprUnary(_) | SemanticKind::ExprBinary(_)
            )
        }
        FinishKind::Evar => selected == SemanticKind::ExprVar,
        FinishKind::OrderedSequence => matches!(selected, SemanticKind::OrderedCollection { .. }),
        FinishKind::Emethod => selected == SemanticKind::EMethod,
        FinishKind::EmapFinish => selected == SemanticKind::EMap,
        FinishKind::ReceivePresentCondition | FinishKind::ReceiveAbsentCondition => {
            selected == SemanticKind::Component(ParComponentTag::Receive)
        }
        FinishKind::Nonsemantic => {
            matches!(selected, SemanticKind::Component(_) | SemanticKind::Expr)
        }
        FinishKind::Var => selected == SemanticKind::Var,
        FinishKind::EpathmapOwner => selected == SemanticKind::EPathMap,
        FinishKind::Ezipper => selected == SemanticKind::EZipper,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParentKind {
    Root,
    Message,
    Map,
    EPathMap,
    CanonicalPath,
}

fn parent_kind_name(parent: ParentKind) -> &'static str {
    match parent {
        ParentKind::Root => "root",
        ParentKind::Message => "message",
        ParentKind::Map => "map",
        ParentKind::EPathMap => "epathmap",
        ParentKind::CanonicalPath => "path",
    }
}

#[derive(Clone, Debug)]
struct FinishRecord {
    step_index: usize,
    complete_index: usize,
    pop_index: usize,
    child_index: Option<usize>,
    program_type_id: u16,
    selected_kind: SemanticKind,
    score_len_before: usize,
    score_len_after: usize,
    frames_before: usize,
    parent: ParentKind,
    kind: Option<FinishKind>,
    model_selected: bool,
    family_complete: bool,
    scored: bool,
    view: FrameView,
}

struct EntryReplay {
    transcript: Vec<u8>,
    well_formed: bool,
}

#[derive(Clone, Debug)]
struct AdmissionInput {
    root_type_id: u16,
    bytes: Vec<u8>,
}

struct AdmissionRun {
    trace: RootFinishTrace,
    finishes: Vec<FinishRecord>,
    caller_summary: Option<RootSummaryState>,
    replay: Vec<EntryReplay>,
}

fn ensure(condition: bool, message: impl FnOnce() -> String) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message())
    }
}

fn index_finishes(trace: &RootFinishTrace) -> Result<Vec<FinishRecord>, String> {
    ensure(
        trace.message_eofs.len() == trace.message_completes.len(),
        || {
            format!(
                "{} message EOF records but {} message completions",
                trace.message_eofs.len(),
                trace.message_completes.len()
            )
        },
    )?;
    let mut pop_of_step = Vec::with_capacity(trace.driver_steps.len());
    let mut pops = 0;
    for step in &trace.driver_steps {
        if step.kind == DriverStepKind::Complete {
            pop_of_step.push(Some(pops));
            pops += 1;
        } else {
            pop_of_step.push(None);
        }
    }
    ensure(pops == trace.pop_accepts.len(), || {
        format!(
            "{pops} completing steps but {} pre-pop records",
            trace.pop_accepts.len()
        )
    })?;
    let mut finishes = Vec::with_capacity(trace.message_completes.len());
    let mut children = 0;
    for success in &trace.message_success {
        match success {
            MessageSuccessState::Eof {
                step_index,
                program_type_id,
                selected_kind,
                score_len_before,
                score_len_after,
                complete_index,
                root,
                ..
            } => {
                let step = trace.driver_steps.get(*step_index).ok_or_else(|| {
                    format!("the EOF receipt names the unrecorded step {step_index}")
                })?;
                let pop_index =
                    pop_of_step
                        .get(*step_index)
                        .copied()
                        .flatten()
                        .ok_or_else(|| {
                            format!("step {step_index} of a message EOF did not complete")
                        })?;
                let (eof, complete) = trace
                    .message_eofs
                    .get(*complete_index)
                    .zip(trace.message_completes.get(*complete_index))
                    .ok_or_else(|| format!("step {step_index} has no EOF and completion record"))?;
                ensure(
                    step.before_top_kind == FrameRequestKind::Message
                        && eof.step_index == *step_index
                        && complete.step_index == *step_index,
                    || format!("the records of step {step_index} do not describe one message EOF"),
                )?;
                let parent = match trace.pop_accepts[pop_index].parent_kind {
                    None => ParentKind::Root,
                    Some(FrameRequestKind::Message) => ParentKind::Message,
                    Some(FrameRequestKind::Map) => ParentKind::Map,
                    Some(FrameRequestKind::EPathMap) => ParentKind::EPathMap,
                    Some(FrameRequestKind::CanonicalPath) => ParentKind::CanonicalPath,
                };
                ensure(*root == (parent == ParentKind::Root), || {
                    format!("step {step_index} returns to the caller but has a parent frame")
                })?;
                let child_index = if *root {
                    None
                } else {
                    children += 1;
                    Some(children - 1)
                };
                let reduced = match child_index {
                    Some(index) => trace.children.get(index).map(|child| &child.summary),
                    None => trace.reduced.as_ref(),
                }
                .ok_or_else(|| format!("step {step_index} recorded no reduced summary"))?;
                let view = frame_view(&eof.frame);
                let kind = view.finish_kind();
                finishes.push(FinishRecord {
                    step_index: *step_index,
                    complete_index: *complete_index,
                    pop_index,
                    child_index,
                    program_type_id: *program_type_id,
                    selected_kind: *selected_kind,
                    score_len_before: *score_len_before,
                    score_len_after: *score_len_after,
                    frames_before: step.before_len,
                    parent,
                    kind,
                    model_selected: view.model_selects(kind),
                    family_complete: view.family_complete(),
                    scored: reduced.score_handle.is_some(),
                    view,
                });
            }
            MessageSuccessState::Accepted {
                step_index,
                pop_index,
                ..
            } => {
                ensure(
                    pop_of_step.get(*step_index).copied().flatten() == Some(*pop_index),
                    || {
                        format!("the accept receipt of step {step_index} names pre-pop record {pop_index}")
                    },
                )?
            }
            MessageSuccessState::Field { .. } => {}
        }
    }
    ensure(
        finishes.len() == trace.message_completes.len() && children == trace.children.len(),
        || {
            format!(
                "{} EOF receipts for {} completions and {} child finishes",
                finishes.len(),
                trace.message_completes.len(),
                trace.children.len()
            )
        },
    )?;
    Ok(finishes)
}

fn check_append_log(trace: &RootFinishTrace) -> Result<(), String> {
    let image = trace
        .score_arena
        .as_deref()
        .ok_or_else(|| "the root return recorded no source image".to_owned())?;
    let mut expected_len = 0;
    for (index, step) in trace.driver_steps.iter().enumerate() {
        ensure(
            step.score_len_before == expected_len
                && step.score_len_after == expected_len + step.score_appended.len()
                && image.get(step.score_len_before..step.score_len_after)
                    == Some(step.score_appended.as_slice()),
            || format!("step {index} does not extend the source image by its appended entries"),
        )?;
        expected_len = step.score_len_after;
    }
    ensure(expected_len == image.len(), || {
        format!(
            "the steps appended {expected_len} entries but the image holds {}",
            image.len()
        )
    })
}

fn admit(input: &AdmissionInput) -> Result<AdmissionRun, String> {
    let program = wire_program_by_type_id(input.root_type_id)
        .map_err(|error| format!("type {} has no wire program: {error:?}", input.root_type_id))?;
    start_with_message_success_stream();
    let source =
        IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(&input.bytes), 0);
    let outcome = drive_admission(
        FrameRequest::Message {
            region: source,
            program,
        },
        input.bytes.len(),
    );
    let trace = take();
    let (summary, scores, _) =
        outcome.map_err(|error| format!("production refused the generated input: {error:?}"))?;
    let caller_summary = frame_summary_state(&summary, &scores);
    check_append_log(&trace)?;
    let finishes = index_finishes(&trace)?;
    Ok(AdmissionRun {
        trace,
        finishes,
        caller_summary,
        replay: Vec::new(),
    })
}

fn atom_transcript(prefix: u8, payload: &[u8]) -> Vec<u8> {
    let mut transcript = Vec::with_capacity(payload.len() * 2 + 4);
    transcript.extend([1, prefix]);
    for &byte in payload {
        transcript.push(byte);
        if byte == 0 {
            transcript.push(0xff);
        }
    }
    transcript.extend([0, 0]);
    transcript
}

fn replay_transcripts(image: &[CanonicalScoreSourceEntry]) -> Vec<EntryReplay> {
    let mut replayed: Vec<EntryReplay> = Vec::with_capacity(image.len());
    for (index, entry) in image.iter().enumerate() {
        let (transcript, children_precede) = match &entry.kind {
            CanonicalScoreSourceKind::Integer(value) => {
                let mut transcript = Vec::with_capacity(10);
                transcript.extend([1, 1]);
                transcript.extend((value.cast_unsigned() ^ (1u64 << 63)).to_be_bytes());
                (transcript, true)
            }
            CanonicalScoreSourceKind::StringOwned(payload)
            | CanonicalScoreSourceKind::StringShared(payload)
            | CanonicalScoreSourceKind::StringRegion(payload)
            | CanonicalScoreSourceKind::StringIndexed(payload) => {
                (atom_transcript(2, payload), true)
            }
            CanonicalScoreSourceKind::BytesOwned(payload)
            | CanonicalScoreSourceKind::BytesShared(payload)
            | CanonicalScoreSourceKind::BytesRegion(payload)
            | CanonicalScoreSourceKind::BytesIndexed(payload) => {
                (atom_transcript(3, payload), true)
            }
            CanonicalScoreSourceKind::Node(children) => {
                let children_precede = children.iter().all(|child| *child < index);
                let capacity = 2 + children
                    .iter()
                    .filter(|child| **child < index)
                    .map(|child| replayed[*child].transcript.len())
                    .sum::<usize>();
                let mut transcript = Vec::with_capacity(capacity);
                transcript.push(2);
                for child in children.iter().filter(|child| **child < index) {
                    transcript.extend_from_slice(&replayed[*child].transcript);
                }
                transcript.push(0);
                (transcript, children_precede)
            }
        };
        let well_formed = children_precede && entry.encoded_len == transcript.len();
        replayed.push(EntryReplay {
            transcript,
            well_formed,
        });
    }
    replayed
}

impl AdmissionRun {
    fn image(&self) -> Result<&[CanonicalScoreSourceEntry], String> {
        self.trace
            .score_arena
            .as_deref()
            .ok_or_else(|| "the root return recorded no source image".to_owned())
    }

    fn eof_frame(&self, finish: &FinishRecord) -> &RootFrameState {
        &self.trace.message_eofs[finish.complete_index].frame
    }

    fn typed_summary(&self, finish: &FinishRecord) -> &RootReturnedState {
        &self.trace.message_completes[finish.complete_index].summary
    }

    fn pop(&self, finish: &FinishRecord) -> &PopAcceptState {
        &self.trace.pop_accepts[finish.pop_index]
    }

    fn reduced_summary(&self, finish: &FinishRecord) -> Result<&RootSummaryState, String> {
        match finish.child_index {
            Some(index) => self.trace.children.get(index).map(|child| &child.summary),
            None => self.trace.reduced.as_ref(),
        }
        .ok_or_else(|| format!("step {} recorded no reduced summary", finish.step_index))
    }

    fn pre_pop_summary(&self, finish: &FinishRecord) -> Result<&RootSummaryState, String> {
        self.pop(finish)
            .summary
            .as_ref()
            .ok_or_else(|| format!("step {} recorded no pre-pop summary", finish.step_index))
    }

    fn owner_of(&self, entry_index: usize) -> Option<&FinishRecord> {
        self.finishes.iter().find(|finish| {
            finish.score_len_before <= entry_index && entry_index < finish.score_len_after
        })
    }

    fn appended_by(&self, finish: &FinishRecord, entry_index: usize) -> bool {
        finish.score_len_before <= entry_index && entry_index < finish.score_len_after
    }

    fn forwarded_origin(&self, finish: &FinishRecord, entry_index: usize) -> Option<&'static str> {
        let [returned] = finish.view.returns.as_slice() else {
            return None;
        };
        let carries_entry = returned.slot == 0
            && returned.handle.map(|handle| handle.index()) == Some(entry_index)
            && entry_index < finish.score_len_before
            && self.owner_of(entry_index).is_some();
        let message_arm = matches!(returned.tag, 5..=24 | 27..=33 | 36 | 37)
            && finish.view.family == ModelFamily::Expr;
        match returned.kind {
            ReturnKind::CostScore
                if carries_entry && finish.program_type_id == 10 && returned.tag == 3 =>
            {
                Some("cost-bound-forward")
            }
            ReturnKind::ExprPayload {
                score_present: true,
                ..
            } if carries_entry && message_arm => Some("expr-payload-forward"),
            _ => None,
        }
    }

    fn origin_label(&self, finish: &FinishRecord, entry_index: usize) -> Option<&'static str> {
        if self.appended_by(finish, entry_index) {
            Some("own")
        } else {
            self.forwarded_origin(finish, entry_index)
        }
    }

    fn binding_semantic(&self, entry_index: usize) -> Option<bool> {
        self.owner_of(entry_index)
            .map(|owner| reported_semantic_flag(self.typed_summary(owner)).unwrap_or(true))
    }

    fn origin_and_semantic(
        &self,
        finish: &FinishRecord,
        entry_index: usize,
        stage: &str,
    ) -> Result<(), String> {
        ensure(self.origin_label(finish, entry_index).is_some(), || {
            format!(
                "{stage}: entry {entry_index} was not appended for this frame and is not a \
                 forwarded child score"
            )
        })?;
        let semantic = self.binding_semantic(entry_index);
        ensure(semantic == Some(finish.family_complete), || {
            format!(
                "{stage}: the semantic flag bound to entry {entry_index} is {semantic:?} but the \
                 frame's completeness is {}",
                finish.family_complete
            )
        })
    }

    fn handle_facts(
        &self,
        finish: &FinishRecord,
        summary: &RootSummaryState,
        arena_len: usize,
        stage: &str,
    ) -> Result<(), String> {
        let handle = summary
            .score_handle
            .ok_or_else(|| format!("{stage}: the frame has no score handle"))?;
        let index = handle.index();
        let image = self.image()?;
        ensure(
            index < arena_len && index < image.len() && index < self.replay.len(),
            || format!("{stage}: handle {index} is outside the arena of {arena_len} entries"),
        )?;
        self.origin_and_semantic(finish, index, stage)?;
        let replayed = &self.replay[index];
        ensure(
            summary.score_encoded_len == Some(image[index].encoded_len),
            || {
                format!(
                "{stage}: production serves length {:?} for entry {index} whose source entry has \
                 length {}",
                summary.score_encoded_len, image[index].encoded_len
            )
            },
        )?;
        ensure(
            summary.score_bytes.as_deref() == Some(replayed.transcript.as_slice()),
            || {
                format!(
                    "{stage}: production serves other bytes for entry {index} than its transcript"
                )
            },
        )?;
        ensure(replayed.well_formed, || {
            format!("{stage}: source entry {index} is not well formed")
        })?;
        ensure(
            self.replay[..arena_len.min(self.replay.len())]
                .iter()
                .all(|entry| entry.well_formed)
                && summary.score_encoded_len == Some(replayed.transcript.len()),
            || format!("{stage}: handle {index} is not bound to the bytes of its transcript"),
        )
    }

    fn score_body_agreement(
        &self,
        finish: &FinishRecord,
        summary: &RootSummaryState,
        arena_len: usize,
        stage: &str,
    ) -> Result<(), String> {
        let handle = summary
            .score_handle
            .ok_or_else(|| format!("{stage}: the frame has no score handle"))?;
        let index = handle.index();
        ensure(index < arena_len, || {
            format!("{stage}: handle {index} is outside the arena of {arena_len} entries")
        })?;
        let encoded_len = summary.score_encoded_len.unwrap_or(0);
        ensure(encoded_len > 0, || {
            format!("{stage}: the binding of entry {index} has no positive encoded length")
        })?;
        ensure(
            summary.score_bytes.as_ref().map(Vec::len) == Some(encoded_len),
            || format!("{stage}: the binding of entry {index} does not hold {encoded_len} bytes"),
        )?;
        self.origin_and_semantic(finish, index, stage)
    }

    fn top_handle_facts(&self, finish: &FinishRecord) -> Result<(), String> {
        let summary = self.reduced_summary(finish)?;
        ensure(summary.score_handle.is_some(), || {
            "the selected finish left no top handle".to_owned()
        })?;
        ensure(
            finish
                .kind
                .is_some_and(|kind| selected_reducer_matches(kind, finish.selected_kind)),
            || {
                format!(
                    "production selected the reducer {:?} for the finish kind {:?}",
                    finish.selected_kind, finish.kind
                )
            },
        )?;
        self.handle_facts(finish, summary, finish.score_len_after, "after the finish")
    }

    fn returned_occurrence(&self, finish: &FinishRecord) -> Result<&RootOccurrenceState, String> {
        let pop = self.pop(finish);
        let (Some(before), Some(after)) = (pop.parent_before.as_ref(), pop.parent_after.as_ref())
        else {
            return Err("the pop recorded no message parent".to_owned());
        };
        let Some(RootWaitState::Child { slot, .. }) = &before.waiting_detail else {
            return Err("the parent was not waiting on a message child".to_owned());
        };
        let gained_one = before.semantic_slots.len() == after.semantic_slots.len()
            && before
                .semantic_slots
                .iter()
                .zip(&after.semantic_slots)
                .enumerate()
                .all(|(index, (earlier, later))| {
                    if index == *slot {
                        later.len() == earlier.len() + 1 && later[..earlier.len()] == earlier[..]
                    } else {
                        later == earlier
                    }
                });
        ensure(gained_one, || {
            "the parent did not gain exactly one occurrence at its waiting slot".to_owned()
        })?;
        after.semantic_slots[*slot]
            .last()
            .ok_or_else(|| "the waiting slot is empty after the pop".to_owned())
    }

    fn child_return_facts(&self, finish: &FinishRecord) -> Result<(), String> {
        let pop = self.pop(finish);
        let child = self.pre_pop_summary(finish)?;
        let returned = self.returned_occurrence(finish)?;
        ensure(
            pop.after_len
                .is_some_and(|frames| frames > 0 && frames + 1 == pop.before_len),
            || {
                format!(
                    "the pop left {:?} of {} frames",
                    pop.after_len, pop.before_len
                )
            },
        )?;
        let RootOccurrenceValue::Message(value) = &returned.value else {
            return Err("the parent's new occurrence is not a message return".to_owned());
        };
        ensure(returned_handle(value) == child.score_handle, || {
            format!(
                "the parent's new return carries handle {:?} but the child finished with {:?}",
                returned_handle(value).map(|handle| handle.index()),
                child.score_handle.map(|handle| handle.index())
            )
        })?;
        let child_frame = pop
            .child_frame
            .as_ref()
            .ok_or_else(|| "the pop recorded no child frame".to_owned())?;
        ensure(
            returned.body_start == Some(child_frame.region_start)
                && returned.body_end == Some(child_frame.region_end),
            || "the parent's new return names another body than the child's".to_owned(),
        )?;
        ensure(
            returned.body.is_some() && returned.body == pop.child_body,
            || "the parent's new return holds other body bytes than the child's".to_owned(),
        )?;
        match child.score_handle {
            None => Ok(()),
            Some(handle) => {
                ensure(self.owner_of(handle.index()).is_some(), || {
                    format!(
                        "no message finish appended entry {} of the return",
                        handle.index()
                    )
                })?;
                self.handle_facts(
                    finish,
                    child,
                    pop.source_len_after.unwrap_or(pop.source_len_before),
                    "after the pop",
                )
            }
        }
    }

    fn pop_keeps_arena(&self, finish: &FinishRecord) -> Result<(), String> {
        let pop = self.pop(finish);
        ensure(pop.source_len_after == Some(pop.source_len_before), || {
            format!(
                "the pop changed the arena from {} to {:?} entries",
                pop.source_len_before, pop.source_len_after
            )
        })
    }

    fn pre_pop_is_after_finish(&self, finish: &FinishRecord) -> Result<(), String> {
        ensure(
            self.pre_pop_summary(finish)? == self.reduced_summary(finish)?
                && self.pop(finish).source_len_before == finish.score_len_after,
            || "the pre-pop state is not the state after the selected finish".to_owned(),
        )
    }

    fn map_value_return(&self, finish: &FinishRecord) -> Result<(), String> {
        let pop = self.pop(finish);
        let child = self.pre_pop_summary(finish)?;
        let (Some(NonMessageFrameState::Map(_)), Some(NonMessageFrameState::Map(after))) =
            (&pop.parent_nonmessage_before, &pop.parent_nonmessage_after)
        else {
            return Err("the pop recorded no map parent".to_owned());
        };
        let Some(RootReturnedState::Par(par)) = &pop.typed_summary else {
            return Err("the map value did not return a Par summary".to_owned());
        };
        ensure(after.value.as_ref() == Some(par), || {
            "the map frame does not hold the returned Par summary".to_owned()
        })?;
        ensure(Some(par.score_handle) == child.score_handle, || {
            "the map frame's value carries another handle than the child's".to_owned()
        })?;
        let body = [
            par.identity_prefix.as_slice(),
            par.identity_suffix.as_slice(),
        ]
        .concat();
        ensure(pop.child_body.as_deref() == Some(body.as_slice()), || {
            "the map frame's value holds other body bytes than the child's".to_owned()
        })?;
        self.handle_facts(
            finish,
            child,
            pop.source_len_after.unwrap_or(pop.source_len_before),
            "after the pop",
        )
    }

    fn child_step_return(&self, finish: &FinishRecord) -> Result<(), String> {
        let pop = self.pop(finish);
        let child = self.pre_pop_summary(finish)?;
        self.pop_keeps_arena(finish)?;
        let typed = pop
            .typed_summary
            .as_ref()
            .ok_or_else(|| "the pop recorded no typed summary".to_owned())?;
        ensure(
            return_kind_has_score(return_kind(typed)) == child.score_handle.is_some(),
            || "the returned summary kind and the child's handle disagree about a score".to_owned(),
        )?;
        match finish.parent {
            ParentKind::Message => {
                let returned = self.returned_occurrence(finish)?;
                ensure(
                    matches!(&returned.value, RootOccurrenceValue::Message(value) if **value == *typed),
                    || "the parent's new return is not the child's summary".to_owned(),
                )?;
                self.child_return_facts(finish)
            }
            ParentKind::Map => self.map_value_return(finish),
            _ => Err("the child returned to neither a message frame nor a map frame".to_owned()),
        }
    }

    fn root_return_facts(&self, finish: &FinishRecord) -> Result<(), String> {
        let returned = self
            .trace
            .returned
            .as_ref()
            .ok_or_else(|| "no root return was recorded".to_owned())?;
        let pre_pop = self
            .trace
            .pre_pop
            .as_ref()
            .ok_or_else(|| "no root pre-pop summary was recorded".to_owned())?;
        ensure(self.trace.remaining_frames == Some(0), || {
            format!(
                "the root return left {:?} frames",
                self.trace.remaining_frames
            )
        })?;
        ensure(
            finish.frames_before == 1 && self.pop(finish).before_len == 1,
            || {
                format!(
                    "the root finished with {} frames on the stack",
                    finish.frames_before
                )
            },
        )?;
        let handle = returned
            .score_handle
            .ok_or_else(|| "the root summary has no score handle".to_owned())?;
        ensure(pre_pop.score_handle == Some(handle), || {
            "the root summary's handle is not the handle of the one pre-pop frame".to_owned()
        })?;
        ensure(
            self.replay.get(handle.index()).is_some_and(|entry| {
                returned.score_bytes.as_deref() == Some(entry.transcript.as_slice())
            }),
            || "the root score bytes are not the transcript of the root handle".to_owned(),
        )?;
        ensure(self.caller_summary.as_ref() == Some(returned), || {
            "the summary handed to the caller is not the recorded root return".to_owned()
        })?;
        self.handle_facts(
            finish,
            pre_pop,
            self.pop(finish).source_len_before,
            "before the root pop",
        )
    }

    fn root_return_premises(&self, finish: &FinishRecord) -> Result<(), String> {
        let (Some(pre_pop), Some(returned)) =
            (self.trace.pre_pop.as_ref(), self.trace.returned.as_ref())
        else {
            return Err("the root recorded no pre-pop summary and return".to_owned());
        };
        let pop = self.pop(finish);
        self.handle_facts(
            finish,
            pre_pop,
            pop.source_len_before,
            "before the root pop",
        )?;
        ensure(
            self.trace.remaining_frames == Some(0)
                && pop.before_len == 1
                && returned.score_handle == pre_pop.score_handle
                && returned.score_bytes == pre_pop.score_bytes,
            || "the returned root summary is not the pre-pop frame's handle and bytes".to_owned(),
        )
    }

    fn attach_agreement(&self, finish: &FinishRecord) -> Result<(), String> {
        let reduced = self.reduced_summary(finish)?;
        let pop = self.pop(finish);
        let attached = self.pre_pop_summary(finish)?;
        ensure(
            pop.before_len == finish.frames_before && pop.before_len > 0,
            || {
                format!(
                    "attaching the handle changed the frame count from {} to {}",
                    finish.frames_before, pop.before_len
                )
            },
        )?;
        ensure(pop.source_len_before == finish.score_len_after, || {
            format!(
                "attaching the handle changed the arena from {} to {} entries",
                finish.score_len_after, pop.source_len_before
            )
        })?;
        ensure(
            attached.score_handle.is_some() && attached.score_handle == reduced.score_handle,
            || "the top frame does not carry the attached handle".to_owned(),
        )?;
        self.score_body_agreement(finish, attached, pop.source_len_before, "after the attach")
    }

    fn eof_outcome_class(&self, finish: &FinishRecord) -> &'static str {
        match (finish.parent, finish.model_selected) {
            (ParentKind::Root, true) => "scored-root",
            (ParentKind::Root, false) => "unscored-root",
            (ParentKind::Message, true) => "scored-child",
            (ParentKind::Message, false) => "unscored-child",
            _ => "outside-the-child-step",
        }
    }

    fn eof_outcome_facts(&self, finish: &FinishRecord) -> Result<(), String> {
        let reduced = self.reduced_summary(finish)?;
        let pop = self.pop(finish);
        ensure(
            self.trace.driver_steps[finish.step_index].kind == DriverStepKind::Complete,
            || "a message EOF did not complete its frame".to_owned(),
        )?;
        ensure(
            finish.model_selected == reduced.score_handle.is_some(),
            || {
                format!(
                    "the model selects {} finish but production left {} handle",
                    if finish.model_selected { "a" } else { "no" },
                    if reduced.score_handle.is_some() {
                        "a"
                    } else {
                        "no"
                    }
                )
            },
        )?;
        if finish.model_selected {
            self.top_handle_facts(finish)?;
            self.pre_pop_is_after_finish(finish)?;
        } else {
            ensure(
                finish.score_len_before == finish.score_len_after
                    && pop.source_len_before == finish.score_len_before
                    && self.pre_pop_summary(finish)?.score_handle.is_none(),
                || "an unscored finish changed the arena or left a handle".to_owned(),
            )?;
        }
        match (finish.parent, finish.model_selected) {
            (ParentKind::Root, true) => self.root_return_facts(finish),
            (ParentKind::Root, false) => ensure(
                self.trace
                    .returned
                    .as_ref()
                    .is_some_and(|returned| returned.score_handle.is_none())
                    && !return_kind_has_score(return_kind(self.typed_summary(finish)))
                    && self.trace.remaining_frames == Some(0),
                || "an unscored root return carries a score or leaves frames".to_owned(),
            ),
            (ParentKind::Message, _) => {
                self.pop_keeps_arena(finish)?;
                self.child_return_facts(finish)
            }
            _ => Ok(()),
        }
    }

    fn epathmap_source_finish(&self, finish: &FinishRecord) -> Result<(), String> {
        let frame = self.eof_frame(finish);
        let snapshot = frame
            .semantic_slots
            .iter()
            .flatten()
            .rev()
            .find_map(|occurrence| match &occurrence.value {
                RootOccurrenceValue::EPathMap { snapshot, .. } => Some(snapshot.as_slice()),
                _ => None,
            })
            .ok_or_else(|| "the owner reached EOF without a returned snapshot".to_owned())?;
        let connective_used = frame.semantic_slots.get(1).is_some_and(|occurrences| {
            occurrences.iter().any(|occurrence| {
                occurrence.value == RootOccurrenceValue::Scalar(RootScalarState::Bool(true))
            })
        });
        let step = &self.trace.driver_steps[finish.step_index];
        let base = finish.score_len_before;
        let appended = base
            .checked_sub(step.score_len_before)
            .and_then(|offset| {
                step.score_appended
                    .get(offset..offset + (finish.score_len_after - base))
            })
            .ok_or_else(|| "the owner's appended entries are outside its step".to_owned())?;
        let constructor_shape = match appended {
            [bytes, flag, constructor, node] => {
                let snapshot_atom = match &bytes.kind {
                    CanonicalScoreSourceKind::BytesOwned(payload)
                    | CanonicalScoreSourceKind::BytesShared(payload)
                    | CanonicalScoreSourceKind::BytesRegion(payload)
                    | CanonicalScoreSourceKind::BytesIndexed(payload) => {
                        payload.as_slice() == snapshot
                    }
                    _ => false,
                };
                snapshot_atom
                    && flag.kind == CanonicalScoreSourceKind::Integer(i64::from(connective_used))
                    && constructor.kind
                        == CanonicalScoreSourceKind::Integer(i64::from(Score::EPATHMAP))
                    && node.kind == CanonicalScoreSourceKind::Node(vec![base + 2, base, base + 1])
            }
            _ => false,
        };
        ensure(
            appended.len() == EPATHMAP_OWNER_APPENDS && constructor_shape,
            || {
                format!(
                "the owner finish appended {:?}, not the snapshot atom, the connective flag and \
                 constructor {} over them",
                appended.iter().map(|entry| &entry.kind).collect::<Vec<_>>(),
                Score::EPATHMAP
            )
            },
        )?;
        ensure(
            self.reduced_summary(finish)?
                .score_handle
                .map(|handle| handle.index())
                == Some(base + EPATHMAP_OWNER_APPENDS - 1),
            || "the owner's handle is not the constructor node of its finish".to_owned(),
        )?;
        self.top_handle_facts(finish)
    }

    fn epathmap_product_finish(&self, finish: &FinishRecord) -> Result<(), String> {
        let score = self
            .trace
            .epathmap_scores
            .iter()
            .find(|score| score.step_index == finish.step_index)
            .ok_or_else(|| "production recorded no EPathMap score for the owner".to_owned())?;
        let accepted = self
            .trace
            .epathmap_accepts
            .iter()
            .rev()
            .find(|accept| {
                accept.parent_type_id == EPATHMAP_OWNER_TYPE_ID
                    && accept.step_index < finish.step_index
            })
            .ok_or_else(|| "the owner accepted no EPathMap source before its EOF".to_owned())?;
        let base = finish.score_len_before;
        ensure(accepted.stored_snapshot == score.snapshot, || {
            "the owner scored another snapshot than the accepted source".to_owned()
        })?;
        ensure(
            score.source_len_before == base
                && score.source_len_after == finish.score_len_after
                && score.source_len_after == base + EPATHMAP_OWNER_APPENDS
                && score.snapshot_handle.index() == base
                && score.score_handle.index() + 1 == score.source_len_after,
            || {
                format!(
                    "the owner's score record spans entries {} to {} with snapshot handle {} and \
                     score handle {}, not the four entries from {base}",
                    score.source_len_before,
                    score.source_len_after,
                    score.snapshot_handle.index(),
                    score.score_handle.index()
                )
            },
        )?;
        ensure(
            self.reduced_summary(finish)?.score_handle == Some(score.score_handle),
            || "the owner's top handle is not the handle of its score record".to_owned(),
        )?;
        self.top_handle_facts(finish)
    }

    fn epathmap_eof_outcome(&self, finish: &FinishRecord) -> Result<(), String> {
        ensure(self.reduced_summary(finish)?.score_handle.is_some(), || {
            "the owner EOF selected no finish".to_owned()
        })?;
        self.epathmap_source_finish(finish)?;
        self.pre_pop_is_after_finish(finish)?;
        match finish.parent {
            ParentKind::Root => self.root_return_facts(finish),
            ParentKind::Message => {
                self.pop_keeps_arena(finish)?;
                self.child_return_facts(finish)
            }
            _ => Err("the owner returned to a frame that is not a message frame".to_owned()),
        }
    }

    fn owner_witness(&self, finish: &FinishRecord) -> Result<(), String> {
        ensure(finish.frames_before == 2, || {
            format!(
                "the owner reached EOF with {} frames, not two",
                finish.frames_before
            )
        })?;
        let below = self
            .pop(finish)
            .parent_before
            .as_ref()
            .ok_or_else(|| "the owner has no message frame below it".to_owned())?;
        let waits_on_owner = below.program_type_id == 24
            && matches!(
                &below.waiting_detail,
                Some(RootWaitState::Child { slot: 0, variant, .. })
                    if occurrence_tag(below.program, 0, *variant) == 32
            );
        ensure(waits_on_owner, || {
            "the frame below the owner is not an Expr waiting on tag 32".to_owned()
        })?;
        ensure(
            finish.model_selected && finish.kind == Some(FinishKind::EpathmapOwner),
            || "the owner's EOF does not select the EPathMap owner finish".to_owned(),
        )?;
        self.top_handle_facts(finish)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Claim {
    HandleFacts,
    TopHandle,
    ChildReturnFacts,
    RootReturnFacts,
    HandleFactsFromAgreement,
    TopHandleFromAgreement,
    AttachAgreement,
    AttachAgreementEverywhere,
    KindTopHandle(FinishKind),
    GroupTopHandle(u8),
    SelectedTopHandle,
    EpathmapSourceFinish,
    EpathmapProductFinish,
    ChildStepReturn,
    PrePopReturn,
    ScoredChildReturn,
    RootReturnFromTopHandle,
    EofOutcome,
    EpathmapEofOutcome,
    LiveRootScore,
    LiveChildScore,
    LiveRootComplete,
    OwnerWitness,
}

fn claim_of(invariant: &str) -> Option<Claim> {
    if let Some(kind) = invariant
        .strip_prefix(KIND_INVARIANT_PREFIX)
        .and_then(finish_kind_named)
    {
        return Some(Claim::KindTopHandle(kind));
    }
    if let Some(class) = invariant
        .strip_prefix(GROUP_INVARIANT_PREFIX)
        .and_then(|digits| digits.parse::<u8>().ok())
    {
        return (1..=FINISH_CLASS_COUNT)
            .contains(&class)
            .then_some(Claim::GroupTopHandle(class));
    }
    match invariant {
        "finish_handle_facts" => Some(Claim::HandleFacts),
        "finish_top_handle" => Some(Claim::TopHandle),
        "finish_child_return_facts" => Some(Claim::ChildReturnFacts),
        "finish_root_return_facts" => Some(Claim::RootReturnFacts),
        "handle_facts_from_agreement" => Some(Claim::HandleFactsFromAgreement),
        "top_handle_from_agreement" => Some(Claim::TopHandleFromAgreement),
        "attach_agreement_holds" => Some(Claim::AttachAgreement),
        "attach_agreement_everywhere" => Some(Claim::AttachAgreementEverywhere),
        "handle_after_selected_finish" => Some(Claim::SelectedTopHandle),
        "handle_after_epm_source_finish" => Some(Claim::EpathmapSourceFinish),
        "handle_after_product_epm_finish" => Some(Claim::EpathmapProductFinish),
        "child_return_carries_handle" => Some(Claim::ChildStepReturn),
        "pre_pop_return_carries_handle" => Some(Claim::PrePopReturn),
        "eof_score_child_carries_handle" => Some(Claim::ScoredChildReturn),
        "root_return_from_top_handle" => Some(Claim::RootReturnFromTopHandle),
        "runtime_eof_finish_facts" => Some(Claim::EofOutcome),
        "runtime_epathmap_eof_finish_facts" => Some(Claim::EpathmapEofOutcome),
        "live_root_score_has_top_handle" => Some(Claim::LiveRootScore),
        "live_child_score_has_top_handle" => Some(Claim::LiveChildScore),
        "live_root_complete_carries_handle" => Some(Claim::LiveRootComplete),
        "handle_owner_witness" => Some(Claim::OwnerWitness),
        _ => None,
    }
}

fn in_domain(claim: Claim, finish: &FinishRecord) -> bool {
    let root = finish.parent == ParentKind::Root;
    let message_child = finish.parent == ParentKind::Message;
    match claim {
        Claim::HandleFacts
        | Claim::HandleFactsFromAgreement
        | Claim::AttachAgreement
        | Claim::AttachAgreementEverywhere => finish.scored,
        Claim::TopHandle | Claim::TopHandleFromAgreement | Claim::SelectedTopHandle => {
            finish.model_selected
        }
        Claim::ChildReturnFacts | Claim::PrePopReturn => message_child,
        Claim::RootReturnFacts | Claim::RootReturnFromTopHandle => root && finish.scored,
        Claim::KindTopHandle(kind) => finish.model_selected && finish.kind == Some(kind),
        Claim::GroupTopHandle(class) => {
            finish.model_selected
                && finish
                    .kind
                    .is_some_and(|kind| finish_kind_class(kind) == class)
        }
        Claim::EpathmapSourceFinish
        | Claim::EpathmapProductFinish
        | Claim::EpathmapEofOutcome
        | Claim::OwnerWitness => finish.program_type_id == EPATHMAP_OWNER_TYPE_ID,
        Claim::ChildStepReturn => message_child || finish.parent == ParentKind::Map,
        Claim::ScoredChildReturn => message_child && finish.model_selected,
        Claim::EofOutcome => true,
        Claim::LiveRootScore | Claim::LiveRootComplete => root && finish.model_selected,
        Claim::LiveChildScore => !root && finish.model_selected,
    }
}

fn check_finish(claim: Claim, run: &AdmissionRun, finish: &FinishRecord) -> Result<(), String> {
    match claim {
        Claim::HandleFacts => run.handle_facts(
            finish,
            run.reduced_summary(finish)?,
            finish.score_len_after,
            "after the finish",
        ),
        Claim::TopHandle
        | Claim::KindTopHandle(_)
        | Claim::GroupTopHandle(_)
        | Claim::SelectedTopHandle
        | Claim::LiveRootScore
        | Claim::LiveChildScore => run.top_handle_facts(finish),
        Claim::ChildReturnFacts => run.child_return_facts(finish),
        Claim::RootReturnFacts => run.root_return_facts(finish),
        Claim::HandleFactsFromAgreement => {
            let summary = run.reduced_summary(finish)?;
            let stage = "after the finish";
            run.score_body_agreement(finish, summary, finish.score_len_after, stage)?;
            run.handle_facts(finish, summary, finish.score_len_after, stage)
        }
        Claim::TopHandleFromAgreement => {
            let summary = run.pre_pop_summary(finish)?;
            let arena_len = run.pop(finish).source_len_before;
            run.score_body_agreement(finish, summary, arena_len, "before the pop")?;
            run.handle_facts(finish, summary, arena_len, "before the pop")
        }
        Claim::AttachAgreement | Claim::AttachAgreementEverywhere => run.attach_agreement(finish),
        Claim::EpathmapSourceFinish => run.epathmap_source_finish(finish),
        Claim::EpathmapProductFinish => run.epathmap_product_finish(finish),
        Claim::ChildStepReturn => run.child_step_return(finish),
        Claim::PrePopReturn => {
            run.pop_keeps_arena(finish)?;
            run.child_return_facts(finish)
        }
        Claim::ScoredChildReturn => {
            run.top_handle_facts(finish)?;
            run.pre_pop_is_after_finish(finish)?;
            run.pop_keeps_arena(finish)?;
            run.child_return_facts(finish)
        }
        Claim::RootReturnFromTopHandle => {
            run.root_return_premises(finish)?;
            run.root_return_facts(finish)
        }
        Claim::EofOutcome => run.eof_outcome_facts(finish),
        Claim::EpathmapEofOutcome => run.epathmap_eof_outcome(finish),
        Claim::LiveRootComplete => {
            run.top_handle_facts(finish)?;
            run.pre_pop_is_after_finish(finish)?;
            run.root_return_facts(finish)
        }
        Claim::OwnerWitness => run.owner_witness(finish),
    }
}

#[derive(Default)]
struct Census {
    cases: usize,
    reached: BTreeMap<String, usize>,
}

impl Census {
    fn count(&mut self, key: String) { *self.reached.entry(key).or_insert(0) += 1; }

    fn reached(&self, key: &str) -> usize { self.reached.get(key).copied().unwrap_or(0) }

    fn report(&self, invariant: &str) -> String {
        let reached = self
            .reached
            .iter()
            .map(|(key, count)| format!("{key}={count}"))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "finish-property-census {invariant} cases={} {reached}",
            self.cases
        )
    }
}

fn check_claim(claim: Claim, run: &AdmissionRun, census: &mut Census) -> Result<(), String> {
    let mut attaches = 0;
    for finish in run
        .finishes
        .iter()
        .filter(|finish| in_domain(claim, finish))
    {
        let position = parent_kind_name(finish.parent);
        let label = finish
            .kind
            .filter(|_| finish.model_selected)
            .map_or("unscored", finish_kind_name);
        check_finish(claim, run, finish).map_err(|reason| {
            format!(
                "{VIOLATION_MARK}the {label} finish of program {} at step {} ({position} \
                 position): {reason}",
                finish.program_type_id, finish.step_index
            )
        })?;
        census.count(format!("{label}@{position}"));
        match claim {
            Claim::EofOutcome => {
                census.count(format!("outcome:{}", run.eof_outcome_class(finish)));
            }
            Claim::HandleFacts => {
                let origin = run
                    .reduced_summary(finish)?
                    .score_handle
                    .and_then(|handle| run.origin_label(finish, handle.index()))
                    .unwrap_or("none");
                census.count(format!("origin:{origin}"));
            }
            Claim::AttachAgreementEverywhere => attaches += 1,
            Claim::OwnerWitness => census.count("witness".to_owned()),
            _ => {}
        }
    }
    if claim == Claim::AttachAgreementEverywhere && attaches >= NESTED_ATTACH_FLOOR {
        census.count("nested-run".to_owned());
    }
    Ok(())
}

const NESTED_ATTACH_FLOOR: usize = 8;
const UNSCORED_VARIANTS: u8 = 19;
const FORGED_PROBE_CASES: u32 = 2;
const SHRINK_ITERATIONS: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position {
    Root,
    Child,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Subject {
    Scored(FinishKind),
    Unscored(u8),
    MapValue,
    Composite,
    Witness,
}

#[derive(Clone, Copy, Debug)]
struct GeneratorRow {
    subject: Subject,
    position: Position,
}

fn all_finish_kinds() -> Vec<FinishKind> { FINISH_KINDS.iter().map(|(kind, _)| *kind).collect() }

fn class_finish_kinds(class: u8) -> Vec<FinishKind> {
    FINISH_KINDS
        .iter()
        .map(|(kind, _)| *kind)
        .filter(|kind| finish_kind_class(*kind) == class)
        .collect()
}

fn subject_rows(subjects: &[Subject], positions: &[Position]) -> Vec<GeneratorRow> {
    let mut rows = Vec::with_capacity(subjects.len() * positions.len());
    for subject in subjects {
        for position in positions {
            rows.push(GeneratorRow {
                subject: *subject,
                position: *position,
            });
        }
    }
    rows
}

fn scored_subjects(kinds: &[FinishKind]) -> Vec<Subject> {
    kinds.iter().map(|kind| Subject::Scored(*kind)).collect()
}

fn unscored_subjects() -> Vec<Subject> { (0..UNSCORED_VARIANTS).map(Subject::Unscored).collect() }

fn rows_of(claim: Claim) -> Vec<GeneratorRow> {
    let both = [Position::Root, Position::Child];
    let every_kind = scored_subjects(&all_finish_kinds());
    match claim {
        Claim::KindTopHandle(kind) => subject_rows(&[Subject::Scored(kind)], &both),
        Claim::GroupTopHandle(class) => {
            subject_rows(&scored_subjects(&class_finish_kinds(class)), &both)
        }
        Claim::HandleFacts
        | Claim::TopHandle
        | Claim::SelectedTopHandle
        | Claim::HandleFactsFromAgreement
        | Claim::TopHandleFromAgreement
        | Claim::AttachAgreement => subject_rows(&every_kind, &both),
        Claim::RootReturnFacts
        | Claim::RootReturnFromTopHandle
        | Claim::LiveRootScore
        | Claim::LiveRootComplete => subject_rows(&every_kind, &[Position::Root]),
        Claim::LiveChildScore | Claim::ScoredChildReturn => {
            subject_rows(&every_kind, &[Position::Child])
        }
        Claim::ChildReturnFacts | Claim::PrePopReturn => {
            subject_rows(&[every_kind, unscored_subjects()].concat(), &[
                Position::Child,
            ])
        }
        Claim::ChildStepReturn => subject_rows(
            &[every_kind, unscored_subjects(), vec![Subject::MapValue]].concat(),
            &[Position::Child],
        ),
        Claim::EofOutcome => subject_rows(&[every_kind, unscored_subjects()].concat(), &both),
        Claim::EpathmapSourceFinish | Claim::EpathmapProductFinish | Claim::EpathmapEofOutcome => {
            let owners = [
                Subject::Scored(FinishKind::EpathmapOwner),
                Subject::Scored(FinishKind::Ezipper),
                Subject::Witness,
            ];
            subject_rows(&owners, &both)
        }
        Claim::AttachAgreementEverywhere => subject_rows(&[Subject::Composite], &[Position::Root]),
        Claim::OwnerWitness => subject_rows(&[Subject::Witness], &[Position::Root]),
    }
}

fn cases_of(claim: Claim) -> u32 {
    match claim {
        Claim::KindTopHandle(_) => 24,
        Claim::GroupTopHandle(_) => 12,
        Claim::EpathmapSourceFinish
        | Claim::EpathmapProductFinish
        | Claim::EpathmapEofOutcome
        | Claim::AttachAgreementEverywhere
        | Claim::OwnerWitness => 32,
        _ => 6,
    }
}

fn kind_census_keys(kinds: &[FinishKind], positions: &[&str]) -> Vec<String> {
    let mut keys = Vec::with_capacity(kinds.len() * positions.len());
    for kind in kinds {
        for position in positions {
            keys.push(format!("{}@{position}", finish_kind_name(*kind)));
        }
    }
    keys
}

fn named_census_keys(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn required_census(claim: Claim) -> Vec<String> {
    let every_kind = all_finish_kinds();
    match claim {
        Claim::KindTopHandle(kind) => kind_census_keys(&[kind], &["root", "message"]),
        Claim::GroupTopHandle(class) => {
            kind_census_keys(&class_finish_kinds(class), &["root", "message"])
        }
        Claim::HandleFacts => [
            kind_census_keys(&every_kind, &["root", "message"]),
            named_census_keys(&[
                "origin:own",
                "origin:cost-bound-forward",
                "origin:expr-payload-forward",
            ]),
        ]
        .concat(),
        Claim::TopHandle
        | Claim::SelectedTopHandle
        | Claim::HandleFactsFromAgreement
        | Claim::TopHandleFromAgreement
        | Claim::AttachAgreement => kind_census_keys(&every_kind, &["root", "message"]),
        Claim::RootReturnFacts
        | Claim::RootReturnFromTopHandle
        | Claim::LiveRootScore
        | Claim::LiveRootComplete => kind_census_keys(&every_kind, &["root"]),
        Claim::LiveChildScore | Claim::ScoredChildReturn => {
            kind_census_keys(&every_kind, &["message"])
        }
        Claim::ChildReturnFacts | Claim::PrePopReturn => [
            kind_census_keys(&every_kind, &["message"]),
            named_census_keys(&["unscored@message"]),
        ]
        .concat(),
        Claim::ChildStepReturn => [
            kind_census_keys(&every_kind, &["message"]),
            named_census_keys(&["unscored@message", "raw_par@map"]),
        ]
        .concat(),
        Claim::EofOutcome => [
            kind_census_keys(&every_kind, &["root", "message"]),
            named_census_keys(&[
                "unscored@root",
                "unscored@message",
                "outcome:scored-root",
                "outcome:unscored-root",
                "outcome:scored-child",
                "outcome:unscored-child",
            ]),
        ]
        .concat(),
        Claim::EpathmapSourceFinish | Claim::EpathmapProductFinish | Claim::EpathmapEofOutcome => {
            kind_census_keys(&[FinishKind::EpathmapOwner], &["root", "message"])
        }
        Claim::AttachAgreementEverywhere => named_census_keys(&["nested-run"]),
        Claim::OwnerWitness => named_census_keys(&["witness"]),
    }
}

#[derive(Clone, Debug)]
struct Draw {
    first: i64,
    second: i64,
    flag: bool,
    other_flag: bool,
    choice: u8,
    count: i32,
    payload: Vec<u8>,
    text: String,
}

fn draws() -> BoxedStrategy<Draw> {
    (
        any::<i32>(),
        any::<i32>(),
        any::<bool>(),
        any::<bool>(),
        any::<u8>(),
        0i32..4,
        proptest::collection::vec(any::<u8>(), 0..6),
        "[a-z]{1,6}",
    )
        .prop_map(
            |(first, second, flag, other_flag, choice, count, payload, text)| Draw {
                first: i64::from(first),
                second: i64::from(second),
                flag,
                other_flag,
                choice,
                count,
                payload,
                text,
            },
        )
        .boxed()
}

fn row_inputs(row: GeneratorRow) -> BoxedStrategy<AdmissionInput> {
    draws()
        .prop_map(move |draw| build_input(row, &draw))
        .boxed()
}

fn rooted<Node: ProtobufNode>(root_type_id: u16, node: &Node) -> AdmissionInput {
    AdmissionInput {
        root_type_id,
        bytes: protobuf_encoder::encode_to_vec(node),
    }
}

fn placed<Node: ProtobufNode, Parent: ProtobufNode>(
    position: Position,
    type_id: u16,
    node: Node,
    parent_type_id: u16,
    parent: impl FnOnce(Node) -> Parent,
) -> AdmissionInput {
    match position {
        Position::Root => rooted(type_id, &node),
        Position::Child => rooted(parent_type_id, &parent(node)),
    }
}

fn expr(instance: ExprInstance) -> Expr {
    Expr {
        expr_instance: Some(instance),
    }
}

fn expr_par(instance: ExprInstance) -> Par { Par::default().with_exprs(vec![expr(instance)]) }

fn integer_par(value: i64) -> Par { expr_par(ExprInstance::GInt(value)) }

fn leaf_par(draw: &Draw) -> Par {
    match draw.choice % 4 {
        0 => integer_par(draw.first),
        1 => expr_par(ExprInstance::GBool(draw.flag)),
        2 => expr_par(ExprInstance::GString(draw.text.clone())),
        _ => expr_par(ExprInstance::GByteArray(draw.payload.clone())),
    }
}

fn bound_var(draw: &Draw) -> Var {
    Var {
        var_instance: Some(VarInstance::BoundVar(draw.count)),
    }
}

fn pathmap(draw: &Draw) -> EPathMap {
    match draw.choice % 3 {
        0 => EPathMap::default(),
        1 => EPathMap::from_set_iter([integer_par(draw.first)], Vec::new(), draw.other_flag, None),
        _ => EPathMap::new_map(
            [(integer_par(draw.first), integer_par(draw.second))],
            Vec::new(),
            draw.other_flag,
            None,
        ),
    }
}

fn zipper(pathmap: Option<EPathMap>, draw: &Draw) -> EZipper {
    EZipper {
        pathmap,
        current_path: Vec::new(),
        is_write_zipper: false,
        locally_free: Vec::new(),
        connective_used: draw.flag,
        cursor_kind: 0,
    }
}

fn ground_signature(draw: &Draw) -> CostSignature {
    CostSignature {
        atoms: vec![CostSignatureAtom {
            form: Some(Form::Ground(draw.payload.clone())),
        }],
    }
}

fn cost_region(draw: &Draw) -> CostRegion {
    CostRegion {
        version: 1,
        start_line: u64::from(draw.choice),
        start_column: 1,
        end_line: u64::from(draw.choice) + 1,
        end_column: 2,
        signer: draw.flag.then(|| ground_signature(draw)),
        ordinal: draw.count.unsigned_abs(),
    }
}

fn receive_bind(draw: &Draw, with_source: bool) -> ReceiveBind {
    ReceiveBind {
        patterns: vec![integer_par(draw.second)],
        source: with_source.then(|| leaf_par(draw)),
        free_count: draw.count,
        ..ReceiveBind::default()
    }
}

fn receive(draw: &Draw, bind: ReceiveBind, with_body: bool, with_condition: bool) -> Receive {
    Receive {
        binds: vec![bind],
        body: with_body.then(|| integer_par(draw.first)),
        persistent: draw.flag,
        bind_count: 1,
        condition: with_condition.then(|| leaf_par(draw)),
        ..Receive::default()
    }
}

fn match_case(draw: &Draw, with_source: bool) -> MatchCase {
    MatchCase {
        pattern: Some(integer_par(draw.second)),
        source: with_source.then(|| leaf_par(draw)),
        free_count: draw.count,
        guard: None,
    }
}

fn key_value(key: i64, value: Option<i64>) -> KeyValuePair {
    KeyValuePair {
        key: Some(integer_par(key)),
        value: value.map(integer_par),
    }
}

fn new_term(draw: &Draw) -> New {
    New {
        bind_count: draw.count,
        p: Some(leaf_par(draw)),
        ..New::default()
    }
}

fn expr_input(position: Position, instance: Option<ExprInstance>) -> AdmissionInput {
    let node = Expr {
        expr_instance: instance,
    };
    placed(position, 24, node, 1, |value| {
        Par::default().with_exprs(vec![value])
    })
}

fn payload_input<Node: ProtobufNode>(
    position: Position,
    type_id: u16,
    node: Node,
    arm: impl FnOnce(Node) -> ExprInstance,
) -> AdmissionInput {
    placed(position, type_id, node, 24, |value| expr(arm(value)))
}

fn connective_input(position: Position, instance: Option<ConnectiveInstance>) -> AdmissionInput {
    let node = Connective {
        connective_instance: instance,
    };
    placed(position, 55, node, 1, |value| {
        Par::default().with_connectives(vec![value])
    })
}

fn unforgeable_input(position: Position, instance: Option<UnfInstance>) -> AdmissionInput {
    let node = GUnforgeable {
        unf_instance: instance,
    };
    placed(position, 60, node, 1, |value| {
        Par::default().with_unforgeables(vec![value])
    })
}

fn new_input(position: Position, node: New) -> AdmissionInput {
    placed(position, 20, node, 1, |value| {
        Par::default().with_news(vec![value])
    })
}

fn receive_input(position: Position, node: Receive) -> AdmissionInput {
    placed(position, 19, node, 1, |value| {
        Par::default().with_receives(vec![value])
    })
}

fn structural_input(
    position: Position,
    draw: &Draw,
    left: Option<Par>,
    right: Option<Par>,
) -> AdmissionInput {
    match draw.choice % 19 {
        0 => payload_input(position, 36, ENot { p: left }, ExprInstance::ENotBody),
        1 => payload_input(position, 37, ENeg { p: left }, ExprInstance::ENegBody),
        2 => payload_input(
            position,
            38,
            EMult {
                p1: left,
                p2: right,
            },
            ExprInstance::EMultBody,
        ),
        3 => payload_input(
            position,
            39,
            EDiv {
                p1: left,
                p2: right,
            },
            ExprInstance::EDivBody,
        ),
        4 => payload_input(
            position,
            40,
            EMod {
                p1: left,
                p2: right,
            },
            ExprInstance::EModBody,
        ),
        5 => payload_input(
            position,
            41,
            EPlus {
                p1: left,
                p2: right,
            },
            ExprInstance::EPlusBody,
        ),
        6 => payload_input(
            position,
            42,
            EMinus {
                p1: left,
                p2: right,
            },
            ExprInstance::EMinusBody,
        ),
        7 => payload_input(
            position,
            43,
            ELt {
                p1: left,
                p2: right,
            },
            ExprInstance::ELtBody,
        ),
        8 => payload_input(
            position,
            44,
            ELte {
                p1: left,
                p2: right,
            },
            ExprInstance::ELteBody,
        ),
        9 => payload_input(
            position,
            45,
            EGt {
                p1: left,
                p2: right,
            },
            ExprInstance::EGtBody,
        ),
        10 => payload_input(
            position,
            46,
            EGte {
                p1: left,
                p2: right,
            },
            ExprInstance::EGteBody,
        ),
        11 => payload_input(
            position,
            47,
            EEq {
                p1: left,
                p2: right,
            },
            ExprInstance::EEqBody,
        ),
        12 => payload_input(
            position,
            48,
            ENeq {
                p1: left,
                p2: right,
            },
            ExprInstance::ENeqBody,
        ),
        13 => payload_input(
            position,
            49,
            EAnd {
                p1: left,
                p2: right,
            },
            ExprInstance::EAndBody,
        ),
        14 => payload_input(
            position,
            50,
            EOr {
                p1: left,
                p2: right,
            },
            ExprInstance::EOrBody,
        ),
        15 => payload_input(
            position,
            51,
            EMatches {
                target: left,
                pattern: right,
            },
            ExprInstance::EMatchesBody,
        ),
        16 => payload_input(
            position,
            52,
            EPercentPercent {
                p1: left,
                p2: right,
            },
            ExprInstance::EPercentPercentBody,
        ),
        17 => payload_input(
            position,
            53,
            EPlusPlus {
                p1: left,
                p2: right,
            },
            ExprInstance::EPlusPlusBody,
        ),
        _ => payload_input(
            position,
            54,
            EMinusMinus {
                p1: left,
                p2: right,
            },
            ExprInstance::EMinusMinusBody,
        ),
    }
}

fn scored_input(kind: FinishKind, position: Position, draw: &Draw) -> AdmissionInput {
    let payload = || draw.payload.clone();
    let text = || draw.text.clone();
    match kind {
        FinishKind::RawPar => placed(position, 1, leaf_par(draw), 15, |chan| Send {
            chan: Some(chan),
            ..Send::default()
        }),
        FinishKind::CostAtomAbsent => {
            placed(position, 10, CostSignatureAtom { form: None }, 9, |atom| {
                CostSignature { atoms: vec![atom] }
            })
        }
        FinishKind::CostAtomBytes => {
            let form = match draw.choice % 3 {
                0 => Form::Ground(payload()),
                1 => Form::Quote(payload()),
                _ => Form::RuntimeName(payload()),
            };
            placed(
                position,
                10,
                CostSignatureAtom { form: Some(form) },
                9,
                |atom| CostSignature { atoms: vec![atom] },
            )
        }
        FinishKind::CostBound => placed(
            position,
            11,
            CostBoundSignature { index: draw.count },
            10,
            |bound| CostSignatureAtom {
                form: Some(Form::Bound(bound)),
            },
        ),
        FinishKind::CostAtomBoundForward => {
            let atom = CostSignatureAtom {
                form: Some(Form::Bound(CostBoundSignature { index: draw.count })),
            };
            placed(position, 10, atom, 9, |atom| CostSignature {
                atoms: vec![atom],
            })
        }
        FinishKind::CostSignature => placed(position, 9, ground_signature(draw), 13, |layer| {
            CostTokenStack {
                layers: vec![layer],
            }
        }),
        FinishKind::CostRegion => placed(position, 12, cost_region(draw), 15, |region| Send {
            chan: Some(integer_par(draw.first)),
            cost_region: Some(region),
            ..Send::default()
        }),
        FinishKind::CostTokenStack => {
            let stack = CostTokenStack {
                layers: vec![ground_signature(draw)],
            };
            placed(position, 13, stack, 1, |value| {
                Par::default().with_cost_token_stacks(vec![value])
            })
        }
        FinishKind::Bundle => {
            let bundle = Bundle {
                body: Some(leaf_par(draw)),
                write_flag: draw.flag,
                read_flag: draw.other_flag,
            };
            placed(position, 8, bundle, 1, |value| {
                Par::default().with_bundles(vec![value])
            })
        }
        FinishKind::If => {
            let conditional = If {
                condition: Some(leaf_par(draw)),
                if_true: Some(integer_par(draw.first)),
                if_false: Some(integer_par(draw.second)),
                ..If::default()
            };
            placed(position, 23, conditional, 1, |value| {
                Par::default().with_conditionals(vec![value])
            })
        }
        FinishKind::Send => {
            let send = Send {
                chan: Some(leaf_par(draw)),
                data: vec![integer_par(draw.second)],
                persistent: draw.flag,
                ..Send::default()
            };
            placed(position, 15, send, 1, |value| {
                Par::default().with_sends(vec![value])
            })
        }
        FinishKind::MatchComponent => {
            let matched = Match {
                target: Some(leaf_par(draw)),
                cases: vec![match_case(draw, true)],
                ..Match::default()
            };
            placed(position, 22, matched, 1, |value| {
                Par::default().with_matches(vec![value])
            })
        }
        FinishKind::AbsentSpecialized => {
            if draw.flag {
                expr_input(position, None)
            } else {
                unforgeable_input(position, None)
            }
        }
        FinishKind::AbsentConnective => connective_input(position, None),
        FinishKind::ReceiveBind => placed(position, 16, receive_bind(draw, true), 19, |bind| {
            receive(draw, bind, true, false)
        }),
        FinishKind::MatchCase => placed(position, 21, match_case(draw, true), 22, |case| Match {
            target: Some(integer_par(draw.first)),
            cases: vec![case],
            ..Match::default()
        }),
        FinishKind::BoolOneof => {
            if draw.flag {
                expr_input(position, Some(ExprInstance::GBool(draw.other_flag)))
            } else {
                let instance = match draw.choice % 5 {
                    0 => ConnectiveInstance::ConnBool(draw.other_flag),
                    1 => ConnectiveInstance::ConnInt(draw.other_flag),
                    2 => ConnectiveInstance::ConnString(draw.other_flag),
                    3 => ConnectiveInstance::ConnUri(draw.other_flag),
                    _ => ConnectiveInstance::ConnByteArray(draw.other_flag),
                };
                connective_input(position, Some(instance))
            }
        }
        FinishKind::ExprAtom => {
            let instance = match draw.choice % 4 {
                0 => ExprInstance::GString(text()),
                1 => ExprInstance::GUri(text()),
                2 => ExprInstance::GByteArray(payload()),
                _ => ExprInstance::GBigInt(payload()),
            };
            expr_input(position, Some(instance))
        }
        FinishKind::ExprNumeric => {
            let instance = if draw.flag {
                ExprInstance::GInt(draw.first)
            } else {
                ExprInstance::GDouble(draw.second.unsigned_abs())
            };
            expr_input(position, Some(instance))
        }
        FinishKind::ExprBigRational => {
            let rational = GBigRational {
                numerator: payload(),
                denominator: vec![draw.choice],
            };
            expr_input(position, Some(ExprInstance::GBigRat(rational)))
        }
        FinishKind::ExprFixedPoint => {
            let fixed = GFixedPoint {
                unscaled: payload(),
                scale: draw.count.unsigned_abs(),
            };
            expr_input(position, Some(ExprInstance::GFixedPoint(fixed)))
        }
        FinishKind::SysAuthToken => unforgeable_input(
            position,
            Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
        ),
        FinishKind::NewEmptyCollections => new_input(position, new_term(draw)),
        FinishKind::NewOneInjection => new_input(position, New {
            injections: BTreeMap::from([(text(), integer_par(draw.second))]),
            ..new_term(draw)
        }),
        FinishKind::NewOneAbsentInjection => new_input(position, New {
            injections: BTreeMap::from([(text(), Par::default())]),
            ..new_term(draw)
        }),
        FinishKind::NewOneUri => new_input(position, New {
            uri: vec![text()],
            ..new_term(draw)
        }),
        FinishKind::NewCollections => {
            let (first_name, second_name) = (format!("{}a", draw.text), format!("{}b", draw.text));
            new_input(position, match draw.choice % 3 {
                0 => New {
                    uri: vec![first_name, second_name],
                    ..new_term(draw)
                },
                1 => New {
                    uri: vec![first_name],
                    injections: BTreeMap::from([(second_name, integer_par(draw.second))]),
                    ..new_term(draw)
                },
                _ => New {
                    injections: BTreeMap::from([
                        (first_name, integer_par(draw.first)),
                        (second_name, integer_par(draw.second)),
                    ]),
                    ..new_term(draw)
                },
            })
        }
        FinishKind::ExprPayloadForward => {
            let instance = match draw.choice % 4 {
                0 => ExprInstance::EListBody(EList {
                    ps: vec![integer_par(draw.first), integer_par(draw.second)],
                    ..EList::default()
                }),
                1 => ExprInstance::ETupleBody(ETuple {
                    ps: vec![leaf_par(draw)],
                    ..ETuple::default()
                }),
                2 => ExprInstance::EVarBody(EVar {
                    v: Some(bound_var(draw)),
                }),
                _ => ExprInstance::ENegBody(ENeg {
                    p: Some(leaf_par(draw)),
                }),
            };
            expr_input(position, Some(instance))
        }
        FinishKind::ExprStructural => structural_input(
            position,
            draw,
            Some(integer_par(draw.first)),
            Some(integer_par(draw.second)),
        ),
        FinishKind::Evar => {
            let variable = EVar {
                v: Some(bound_var(draw)),
            };
            payload_input(position, 35, variable, ExprInstance::EVarBody)
        }
        FinishKind::OrderedSequence => match draw.choice % 3 {
            0 => {
                let list = EList {
                    ps: vec![integer_par(draw.second), integer_par(draw.first)],
                    ..EList::default()
                };
                payload_input(position, 27, list, ExprInstance::EListBody)
            }
            1 => {
                let tuple = ETuple {
                    ps: vec![leaf_par(draw), integer_par(draw.second)],
                    ..ETuple::default()
                };
                payload_input(position, 28, tuple, ExprInstance::ETupleBody)
            }
            _ => {
                let set = ESet {
                    ps: vec![integer_par(draw.first), integer_par(draw.first + 1)],
                    ..ESet::default()
                };
                payload_input(position, 29, set, ExprInstance::ESetBody)
            }
        },
        FinishKind::Emethod => {
            let method = EMethod {
                method_name: text(),
                target: Some(leaf_par(draw)),
                arguments: vec![integer_par(draw.second)],
                ..EMethod::default()
            };
            payload_input(position, 33, method, ExprInstance::EMethodBody)
        }
        FinishKind::EmapFinish => {
            let map = EMap {
                kvs: vec![
                    key_value(draw.first, Some(draw.second)),
                    key_value(draw.first + 1, Some(draw.first)),
                ],
                ..EMap::default()
            };
            payload_input(position, 30, map, ExprInstance::EMapBody)
        }
        FinishKind::ConnectiveNot => connective_input(
            position,
            Some(ConnectiveInstance::ConnNotBody(leaf_par(draw))),
        ),
        FinishKind::ConnectiveVarref => {
            let reference = VarRef {
                index: draw.count,
                depth: draw.count + 1,
            };
            connective_input(position, Some(ConnectiveInstance::VarRefBody(reference)))
        }
        FinishKind::ConnectiveCollection => {
            let body = ConnectiveBody {
                ps: vec![leaf_par(draw)],
            };
            connective_input(
                position,
                Some(if draw.flag {
                    ConnectiveInstance::ConnAndBody(body)
                } else {
                    ConnectiveInstance::ConnOrBody(body)
                }),
            )
        }
        FinishKind::UnforgeableBytes => {
            let instance = match draw.choice % 3 {
                0 => UnfInstance::GPrivateBody(GPrivate { id: payload() }),
                1 => UnfInstance::GDeployIdBody(GDeployId { sig: payload() }),
                _ => UnfInstance::GDeployerIdBody(GDeployerId {
                    public_key: payload(),
                }),
            };
            unforgeable_input(position, Some(instance))
        }
        FinishKind::ReceivePresentCondition => receive_input(
            position,
            receive(draw, receive_bind(draw, true), true, true),
        ),
        FinishKind::ReceiveAbsentCondition => receive_input(
            position,
            receive(draw, receive_bind(draw, true), true, false),
        ),
        FinishKind::Nonsemantic => match draw.choice % 7 {
            0 => {
                let send = Send {
                    persistent: draw.flag,
                    ..Send::default()
                };
                placed(position, 15, send, 1, |value| {
                    Par::default().with_sends(vec![value])
                })
            }
            1 => receive_input(
                position,
                receive(draw, receive_bind(draw, true), false, false),
            ),
            2 => new_input(position, New {
                bind_count: draw.count + 1,
                ..New::default()
            }),
            3 => {
                let matched = Match {
                    cases: vec![match_case(draw, true)],
                    ..Match::default()
                };
                placed(position, 22, matched, 1, |value| {
                    Par::default().with_matches(vec![value])
                })
            }
            4 => {
                let bundle = Bundle {
                    body: None,
                    write_flag: true,
                    read_flag: draw.flag,
                };
                placed(position, 8, bundle, 1, |value| {
                    Par::default().with_bundles(vec![value])
                })
            }
            5 => {
                let conditional = If {
                    condition: Some(leaf_par(draw)),
                    ..If::default()
                };
                placed(position, 23, conditional, 1, |value| {
                    Par::default().with_conditionals(vec![value])
                })
            }
            _ => expr_input(position, Some(ExprInstance::ENegBody(ENeg { p: None }))),
        },
        FinishKind::Var => {
            let variable = Var {
                var_instance: match draw.choice % 4 {
                    0 => Some(VarInstance::BoundVar(draw.count)),
                    1 => Some(VarInstance::FreeVar(draw.count)),
                    2 => Some(VarInstance::Wildcard(WildcardMsg {})),
                    _ => None,
                },
            };
            placed(position, 6, variable, 35, |value| EVar { v: Some(value) })
        }
        FinishKind::EpathmapOwner => {
            payload_input(position, 31, pathmap(draw), ExprInstance::EPathmapBody)
        }
        FinishKind::Ezipper => payload_input(
            position,
            32,
            zipper(Some(pathmap(draw)), draw),
            ExprInstance::EZipperBody,
        ),
    }
}

fn unscored_input(variant: u8, position: Position, draw: &Draw) -> AdmissionInput {
    let payload = || draw.payload.clone();
    match variant {
        0 => placed(
            position,
            34,
            key_value(draw.first, Some(draw.second)),
            30,
            |pair| EMap {
                kvs: vec![pair],
                ..EMap::default()
            },
        ),
        1 => {
            let rational = GBigRational {
                numerator: payload(),
                denominator: vec![draw.choice],
            };
            placed(position, 25, rational, 24, |value| {
                expr(ExprInstance::GBigRat(value))
            })
        }
        2 => {
            let fixed = GFixedPoint {
                unscaled: payload(),
                scale: draw.count.unsigned_abs(),
            };
            placed(position, 26, fixed, 24, |value| {
                expr(ExprInstance::GFixedPoint(value))
            })
        }
        3 => {
            let reference = VarRef {
                index: draw.count,
                depth: draw.count + 1,
            };
            placed(position, 56, reference, 55, |value| Connective {
                connective_instance: Some(ConnectiveInstance::VarRefBody(value)),
            })
        }
        4 => {
            let body = ConnectiveBody {
                ps: vec![leaf_par(draw)],
            };
            placed(position, 57, body, 55, |value| Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(value)),
            })
        }
        5 => placed(position, 61, GPrivate { id: payload() }, 60, |value| {
            GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(value)),
            }
        }),
        6 => placed(position, 62, GDeployId { sig: payload() }, 60, |value| {
            GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployIdBody(value)),
            }
        }),
        7 => {
            let deployer = GDeployerId {
                public_key: payload(),
            };
            placed(position, 63, deployer, 60, |value| GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployerIdBody(value)),
            })
        }
        8 => placed(position, 64, GSysAuthToken {}, 60, |value| GUnforgeable {
            unf_instance: Some(UnfInstance::GSysAuthTokenBody(value)),
        }),
        9 => placed(position, 7, WildcardMsg {}, 6, |value| Var {
            var_instance: Some(VarInstance::Wildcard(value)),
        }),
        10 => placed(position, 16, receive_bind(draw, false), 19, |bind| {
            receive(draw, bind, true, false)
        }),
        11 => placed(position, 21, match_case(draw, false), 22, |case| Match {
            target: Some(integer_par(draw.first)),
            cases: vec![case],
            ..Match::default()
        }),
        12 => payload_input(position, 35, EVar { v: None }, ExprInstance::EVarBody),
        13 => {
            let method = EMethod {
                method_name: draw.text.clone(),
                arguments: vec![integer_par(draw.second)],
                ..EMethod::default()
            };
            payload_input(position, 33, method, ExprInstance::EMethodBody)
        }
        14 => {
            let map = EMap {
                kvs: vec![key_value(draw.first, None)],
                ..EMap::default()
            };
            payload_input(position, 30, map, ExprInstance::EMapBody)
        }
        15 => payload_input(position, 32, zipper(None, draw), ExprInstance::EZipperBody),
        16 => structural_input(position, draw, None, None),
        17 => structural_input(position, draw, None, Some(integer_par(draw.second))),
        _ => {
            let body = ParWithRandom {
                body: Some(leaf_par(draw)),
                random_state: payload(),
            };
            placed(position, 3, body, 2, |value| TaggedContinuation {
                tagged_cont: Some(TaggedCont::ParBody(value)),
                ..TaggedContinuation::default()
            })
        }
    }
}

fn map_value_input(position: Position, draw: &Draw) -> AdmissionInput {
    new_input(position, New {
        bind_count: draw.count,
        p: Some(integer_par(draw.first)),
        injections: BTreeMap::from([(draw.text.clone(), leaf_par(draw))]),
        ..New::default()
    })
}

fn composite_input(draw: &Draw) -> AdmissionInput {
    let list = EList {
        ps: vec![integer_par(draw.first), leaf_par(draw)],
        ..EList::default()
    };
    let bundle = Bundle {
        body: Some(integer_par(draw.second)),
        write_flag: draw.flag,
        read_flag: draw.other_flag,
    };
    let datum = Par::default()
        .with_exprs(vec![expr(ExprInstance::EListBody(list))])
        .with_bundles(vec![bundle]);
    let send = Send {
        chan: Some(leaf_par(draw)),
        data: vec![datum],
        persistent: draw.flag,
        ..Send::default()
    };
    let new = New {
        bind_count: draw.count,
        p: Some(Par::default().with_sends(vec![send])),
        uri: vec![draw.text.clone()],
        ..New::default()
    };
    rooted(1, &Par::default().with_news(vec![new]))
}

fn build_input(row: GeneratorRow, draw: &Draw) -> AdmissionInput {
    match row.subject {
        Subject::Scored(kind) => scored_input(kind, row.position, draw),
        Subject::Unscored(variant) => unscored_input(variant, row.position, draw),
        Subject::MapValue => map_value_input(row.position, draw),
        Subject::Composite => composite_input(draw),
        Subject::Witness => rooted(24, &expr(ExprInstance::EPathmapBody(pathmap(draw)))),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Forgery {
    DropReducedHandle,
    ShiftReducedLength,
    FlipReducedByte,
    FlipReportedFlag,
    DetachOwnAppend,
    FlipPrePopByte,
    DropPrePopHandle,
    ShiftPrePopArena,
    GrowArenaAtPop,
    DropReturnedHandle,
    ChangeReturnedBody,
    KeepRootFrame,
    FlipRootReturnByte,
    DropRootPrePopHandle,
    ShiftImageLength,
    ChangeEpathmapConstructor,
    FlipEpathmapFlagEntry,
    DeepenWitness,
}

fn forgery_of(claim: Claim) -> Forgery {
    match claim {
        Claim::HandleFacts | Claim::GroupTopHandle(5) => Forgery::ShiftImageLength,
        Claim::TopHandle
        | Claim::KindTopHandle(_)
        | Claim::EpathmapEofOutcome
        | Claim::GroupTopHandle(6) => Forgery::DropReducedHandle,
        Claim::ChildReturnFacts | Claim::ScoredChildReturn => Forgery::DropReturnedHandle,
        Claim::RootReturnFacts => Forgery::KeepRootFrame,
        Claim::TopHandleFromAgreement => Forgery::FlipPrePopByte,
        Claim::AttachAgreement => Forgery::DropPrePopHandle,
        Claim::AttachAgreementEverywhere => Forgery::ShiftPrePopArena,
        Claim::GroupTopHandle(2) => Forgery::FlipReportedFlag,
        Claim::GroupTopHandle(3) | Claim::LiveRootScore => Forgery::ShiftReducedLength,
        Claim::GroupTopHandle(4) => Forgery::DetachOwnAppend,
        Claim::GroupTopHandle(_)
        | Claim::HandleFactsFromAgreement
        | Claim::SelectedTopHandle
        | Claim::LiveChildScore => Forgery::FlipReducedByte,
        Claim::EpathmapSourceFinish => Forgery::ChangeEpathmapConstructor,
        Claim::EpathmapProductFinish => Forgery::FlipEpathmapFlagEntry,
        Claim::ChildStepReturn => Forgery::GrowArenaAtPop,
        Claim::PrePopReturn => Forgery::ChangeReturnedBody,
        Claim::RootReturnFromTopHandle => Forgery::FlipRootReturnByte,
        Claim::EofOutcome | Claim::LiveRootComplete => Forgery::DropRootPrePopHandle,
        Claim::OwnerWitness => Forgery::DeepenWitness,
    }
}

fn drop_score(summary: &mut RootSummaryState) {
    summary.score_handle = None;
    summary.score_encoded_len = None;
    summary.score_bytes = None;
}

fn reduced_summary_mut<'trace>(
    trace: &'trace mut RootFinishTrace,
    finish: &FinishRecord,
) -> Option<&'trace mut RootSummaryState> {
    match finish.child_index {
        Some(index) => trace
            .children
            .get_mut(index)
            .map(|child| &mut child.summary),
        None => trace.reduced.as_mut(),
    }
}

fn returned_occurrence_mut(pop: &mut PopAcceptState) -> Option<&mut RootOccurrenceState> {
    let slot = match pop
        .parent_before
        .as_ref()
        .and_then(|parent| parent.waiting_detail.as_ref())
    {
        Some(RootWaitState::Child { slot, .. }) => *slot,
        _ => return None,
    };
    pop.parent_after
        .as_mut()
        .and_then(|parent| parent.semantic_slots.get_mut(slot))
        .and_then(|occurrences| occurrences.last_mut())
}

fn forge_observation(claim: Claim, run: &mut AdmissionRun) {
    let Some(target) = run
        .finishes
        .iter()
        .position(|finish| in_domain(claim, finish))
    else {
        return;
    };
    let finish = run.finishes[target].clone();
    let trace = &mut run.trace;
    match forgery_of(claim) {
        Forgery::DropReducedHandle => {
            if let Some(summary) = reduced_summary_mut(trace, &finish) {
                drop_score(summary);
            }
        }
        Forgery::ShiftReducedLength => {
            if let Some(length) = reduced_summary_mut(trace, &finish)
                .and_then(|summary| summary.score_encoded_len.as_mut())
            {
                *length += 1;
            }
        }
        Forgery::FlipReducedByte => {
            if let Some(byte) = reduced_summary_mut(trace, &finish)
                .and_then(|summary| summary.score_bytes.as_mut())
                .and_then(|bytes| bytes.first_mut())
            {
                *byte ^= 1;
            }
        }
        Forgery::FlipReportedFlag => {
            match &mut trace.message_completes[finish.complete_index].summary {
                RootReturnedState::Component(component, _) => {
                    component.complete = !component.complete;
                }
                RootReturnedState::ReceiveBind { source_present, .. } => {
                    *source_present = !*source_present;
                }
                RootReturnedState::MatchCase {
                    required_present, ..
                } => *required_present = !*required_present,
                _ => {}
            }
        }
        Forgery::DetachOwnAppend => {
            run.finishes[target].score_len_before = finish.score_len_after;
        }
        Forgery::FlipPrePopByte => {
            if let Some(byte) = trace.pop_accepts[finish.pop_index]
                .summary
                .as_mut()
                .and_then(|summary| summary.score_bytes.as_mut())
                .and_then(|bytes| bytes.first_mut())
            {
                *byte ^= 1;
            }
        }
        Forgery::DropPrePopHandle => {
            if let Some(summary) = trace.pop_accepts[finish.pop_index].summary.as_mut() {
                drop_score(summary);
            }
        }
        Forgery::ShiftPrePopArena => trace.pop_accepts[finish.pop_index].source_len_before += 1,
        Forgery::GrowArenaAtPop => {
            let pop = &mut trace.pop_accepts[finish.pop_index];
            pop.source_len_after = Some(pop.source_len_before + 1);
        }
        Forgery::DropReturnedHandle => {
            if let Some(occurrence) =
                returned_occurrence_mut(&mut trace.pop_accepts[finish.pop_index])
            {
                occurrence.value =
                    RootOccurrenceValue::Message(Box::new(RootReturnedState::Opaque));
            }
        }
        Forgery::ChangeReturnedBody => {
            if let Some(body) = returned_occurrence_mut(&mut trace.pop_accepts[finish.pop_index])
                .and_then(|occurrence| occurrence.body.as_mut())
            {
                body.push(0xff);
            }
        }
        Forgery::KeepRootFrame => trace.remaining_frames = Some(1),
        Forgery::FlipRootReturnByte => {
            if let Some(byte) = trace
                .returned
                .as_mut()
                .and_then(|summary| summary.score_bytes.as_mut())
                .and_then(|bytes| bytes.first_mut())
            {
                *byte ^= 1;
            }
        }
        Forgery::DropRootPrePopHandle => {
            if let Some(summary) = trace.pre_pop.as_mut() {
                drop_score(summary);
            }
        }
        Forgery::ShiftImageLength => {
            let entry_index = reduced_summary_mut(trace, &finish)
                .and_then(|summary| summary.score_handle)
                .map(|handle| handle.index());
            if let Some(entry) = entry_index.and_then(|index| {
                trace
                    .score_arena
                    .as_mut()
                    .and_then(|image| image.get_mut(index))
            }) {
                entry.encoded_len += 1;
            }
        }
        Forgery::ChangeEpathmapConstructor => {
            if let Some(entry) = trace
                .score_arena
                .as_mut()
                .and_then(|image| image.get_mut(finish.score_len_before + 2))
            {
                entry.kind = CanonicalScoreSourceKind::Integer(i64::from(Score::EPATHMAP) + 1);
            }
        }
        Forgery::FlipEpathmapFlagEntry => {
            if let Some(CanonicalScoreSourceKind::Integer(flag)) = trace
                .score_arena
                .as_mut()
                .and_then(|image| image.get_mut(finish.score_len_before + 1))
                .map(|entry| &mut entry.kind)
            {
                *flag ^= 1;
            }
        }
        Forgery::DeepenWitness => run.finishes[target].frames_before += 1,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Budget {
    Full,
    Probe,
}

struct PropertyFailure {
    violations: Vec<String>,
    obstacles: Vec<String>,
}

impl PropertyFailure {
    fn is_empty(&self) -> bool { self.violations.is_empty() && self.obstacles.is_empty() }

    fn report(&self, invariant: &str) -> String {
        format!(
            "finish invariant {invariant} does not hold on the observed admission:\n{}",
            [self.violations.as_slice(), self.obstacles.as_slice()]
                .concat()
                .join("\n")
        )
    }
}

fn property_seed(invariant: &str, row_index: usize) -> u64 {
    let name_hash = invariant
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    name_hash
        ^ u64::try_from(row_index)
            .unwrap_or(u64::MAX)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

fn property_config(cases: u32, shrink_iterations: u32, seed: u64) -> ProptestConfig {
    let mut config = ProptestConfig {
        cases,
        failure_persistence: None,
        max_shrink_iters: shrink_iterations,
        ..ProptestConfig::default()
    };
    if std::env::var_os("PROPTEST_RNG_SEED").is_none() {
        config.rng_seed = RngSeed::Fixed(seed);
    }
    config
}

fn run_finish_property(
    invariant: &str,
    observed: Observed,
    budget: Budget,
) -> Result<Census, PropertyFailure> {
    let mut failure = PropertyFailure {
        violations: Vec::new(),
        obstacles: Vec::new(),
    };
    let Some(claim) = claim_of(invariant) else {
        failure.obstacles.push(format!(
            "{invariant} names no finish invariant with a runtime check"
        ));
        return Err(failure);
    };
    let rows = rows_of(claim);
    let (cases, shrink_iterations, failure_limit) = match budget {
        Budget::Full => (cases_of(claim), SHRINK_ITERATIONS, REPORTED_ROW_FAILURES),
        Budget::Probe => (FORGED_PROBE_CASES, 0, 1),
    };
    let census = RefCell::new(Census::default());
    for (row_index, row) in rows.iter().enumerate() {
        let seed = property_seed(invariant, row_index);
        let mut runner = TestRunner::new(property_config(cases, shrink_iterations, seed));
        let outcome = runner.run(&row_inputs(*row), |input| {
            let mut run = admit(&input).map_err(TestCaseError::fail)?;
            if observed == Observed::Forged {
                forge_observation(claim, &mut run);
            }
            run.replay = run
                .trace
                .score_arena
                .as_deref()
                .map(replay_transcripts)
                .unwrap_or_default();
            let mut census = census.borrow_mut();
            census.cases += 1;
            check_claim(claim, &run, &mut census).map_err(TestCaseError::fail)
        });
        match outcome {
            Ok(()) => {}
            Err(TestError::Fail(reason, input)) => {
                let message = format!(
                    "{:?} at {:?} position: {reason}; minimal input: root type {}, bytes {:02x?}",
                    row.subject, row.position, input.root_type_id, input.bytes
                );
                if reason.message().starts_with(VIOLATION_MARK) {
                    failure.violations.push(message);
                } else {
                    failure.obstacles.push(message);
                }
            }
            Err(TestError::Abort(reason)) => failure.obstacles.push(format!(
                "{:?} at {:?} position: aborted: {reason}",
                row.subject, row.position
            )),
        }
        if failure.violations.len() + failure.obstacles.len() >= failure_limit {
            break;
        }
    }
    let census = census.into_inner();
    if failure.is_empty() {
        let missing = required_census(claim)
            .into_iter()
            .filter(|key| census.reached(key) == 0)
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            failure.obstacles.push(format!(
                "the generated inputs never reached {}",
                missing.join(", ")
            ));
        }
    }
    if failure.is_empty() {
        Ok(census)
    } else {
        Err(failure)
    }
}

fn check_finish_invariant(invariant: &str, observed: Observed) {
    match run_finish_property(invariant, observed, Budget::Full) {
        Ok(census) => println!("{}", census.report(invariant)),
        Err(failure) => panic!("{}", failure.report(invariant)),
    }
}

fn declared_finish_invariants(source: &str) -> Vec<&str> {
    source
        .lines()
        .filter_map(|line| {
            let declaration = line.trim_start();
            declaration
                .strip_prefix("pub open spec fn ")
                .or_else(|| declaration.strip_prefix("pub closed spec fn "))
                .or_else(|| declaration.strip_prefix("pub proof fn "))
        })
        .filter_map(|signature| signature.split('(').next())
        .collect()
}

struct TestDefinition {
    definitions: usize,
    body: String,
}

fn test_definitions(source: &str) -> BTreeMap<&str, TestDefinition> {
    let mut tests: BTreeMap<&str, TestDefinition> = BTreeMap::new();
    let mut carries_test_attribute = false;
    let mut open_body: Option<&str> = None;
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if let Some(test) = open_body {
            if raw_line == "}" {
                open_body = None;
            } else if let Some(first) = tests
                .get_mut(test)
                .filter(|definition| definition.definitions == 1)
            {
                first.body.extend(line.split_whitespace());
            }
            continue;
        }
        match line {
            "#[test]" => carries_test_attribute = true,
            attribute if attribute.starts_with("#[") => {}
            definition => {
                let signature = definition
                    .strip_prefix("fn ")
                    .filter(|_| carries_test_attribute)
                    .and_then(|signature| signature.split_once('('));
                carries_test_attribute = false;
                if let Some((test, rest)) = signature {
                    let entry = tests.entry(test).or_insert_with(|| TestDefinition {
                        definitions: 0,
                        body: String::new(),
                    });
                    entry.definitions += 1;
                    match rest.split_once('{') {
                        Some((_, body)) if rest.ends_with('}') => {
                            if entry.definitions == 1 {
                                entry
                                    .body
                                    .extend(body.trim_end_matches('}').split_whitespace());
                            }
                        }
                        _ => open_body = Some(test),
                    }
                }
            }
        }
    }
    tests
}

fn test_checks_invariant(test: &TestDefinition, invariant: &str) -> bool {
    test.body
        .starts_with(format!("{FINISH_PROPERTY_ENTRY}(\"{invariant}\",Observed::").as_str())
}

fn binding_defects(
    declared: &[&str],
    tests: &BTreeMap<&str, TestDefinition>,
    bindings: &[(&str, &str)],
) -> Vec<String> {
    let definitions_of = |property: &str| tests.get(property).map_or(0, |test| test.definitions);
    let mut defects = Vec::new();
    for invariant in declared {
        match bindings
            .iter()
            .filter(|(bound, _)| bound == invariant)
            .count()
        {
            1 => {}
            0 => defects.push(format!(
                "finish invariant {invariant} has no bound property"
            )),
            rows => {
                defects.push(format!(
                    "finish invariant {invariant} has {rows} bound properties"
                ));
            }
        }
    }
    for (invariant, _) in bindings {
        if !declared.contains(invariant) {
            defects.push(format!(
                "bound invariant {invariant} is not declared by the Verus file"
            ));
        } else if claim_of(invariant).is_none() {
            defects.push(format!("bound invariant {invariant} has no runtime check"));
        }
    }
    for (_, property) in bindings {
        match definitions_of(property) {
            1 => {}
            definitions => defects.push(format!(
                "bound property {property} is defined {definitions} times as a test function"
            )),
        }
    }
    let mut counted = BTreeSet::new();
    for (_, property) in bindings {
        let rows = bindings
            .iter()
            .filter(|(_, other)| other == property)
            .count();
        if counted.insert(*property) && rows != 1 {
            defects.push(format!("bound property {property} is bound by {rows} rows"));
        }
    }
    for (invariant, property) in bindings {
        let defined_once = tests.get(property).filter(|test| test.definitions == 1);
        if declared.contains(invariant)
            && defined_once.is_some_and(|test| !test_checks_invariant(test, invariant))
        {
            defects.push(format!(
                "bound property {property} does not check finish invariant {invariant}"
            ));
        }
    }
    defects
}

#[test]
fn every_finish_invariant_has_exactly_one_bound_property() {
    let declared = declared_finish_invariants(FINISH_INVARIANT_SOURCE);
    let tests = test_definitions(FINISH_PROPERTY_SOURCE);
    let bindings = FINISH_INVARIANT_BINDINGS;
    let no_defects = Vec::<String>::new();
    assert_eq!(
        binding_defects(&declared, &tests, bindings),
        no_defects,
        "the Verus file, the binding table and the bound properties are out of step"
    );

    for (removed, (invariant, _)) in bindings.iter().enumerate() {
        let remaining = [&bindings[..removed], &bindings[removed + 1..]].concat();
        assert_eq!(binding_defects(&declared, &tests, &remaining), vec![
            format!("finish invariant {invariant} has no bound property")
        ]);
    }

    let extended = [declared.as_slice(), &["some_new_finish_lemma"]].concat();
    assert_eq!(binding_defects(&extended, &tests, bindings), vec![
        "finish invariant some_new_finish_lemma has no bound property".to_owned()
    ]);

    let (first_invariant, first_property) = bindings[0];
    let (second_invariant, second_property) = bindings[1];

    let mut misbound = bindings.to_vec();
    misbound[0].1 = "finish_property_that_is_not_defined";
    assert_eq!(binding_defects(&declared, &tests, &misbound), vec![
        "bound property finish_property_that_is_not_defined is defined 0 times as a test function"
            .to_owned()
    ]);

    let doubled = [bindings, &bindings[..1]].concat();
    assert_eq!(binding_defects(&declared, &tests, &doubled), vec![
        format!("finish invariant {first_invariant} has 2 bound properties"),
        format!("bound property {first_property} is bound by 2 rows"),
    ]);

    let retired = [bindings, &[("retired_finish_lemma", second_property)]].concat();
    assert_eq!(binding_defects(&declared, &tests, &retired), vec![
        "bound invariant retired_finish_lemma is not declared by the Verus file".to_owned(),
        format!("bound property {second_property} is bound by 2 rows"),
    ]);

    let mut exchanged = bindings.to_vec();
    exchanged[0].1 = second_property;
    exchanged[1].1 = first_property;
    assert_eq!(binding_defects(&declared, &tests, &exchanged), vec![
        format!(
            "bound property {second_property} does not check finish invariant {first_invariant}"
        ),
        format!(
            "bound property {first_property} does not check finish invariant {second_invariant}"
        ),
    ]);

    let redefined = format!("{FINISH_PROPERTY_SOURCE}\n#[test]\nfn {first_property}() {{}}\n");
    assert_eq!(
        binding_defects(&declared, &test_definitions(&redefined), bindings),
        vec![format!(
            "bound property {first_property} is defined 2 times as a test function"
        )]
    );
}

#[test]
fn every_finish_property_rejects_its_forged_observation() {
    let mut undetected = Vec::with_capacity(FINISH_INVARIANT_BINDINGS.len());
    for (invariant, property) in FINISH_INVARIANT_BINDINGS {
        match run_finish_property(invariant, Observed::Forged, Budget::Probe) {
            Err(failure) if !failure.violations.is_empty() && failure.obstacles.is_empty() => {}
            Err(failure) => undetected.push(format!(
                "{property}: the forged run met an obstacle: {}",
                failure.report(invariant)
            )),
            Ok(_) => undetected.push(format!(
                "{property}: the forged observation of {invariant} passed"
            )),
        }
    }
    assert!(undetected.is_empty(), "{}", undetected.join("\n"));
}

#[test]
fn scored_finish_handle_has_the_handle_facts() {
    check_finish_invariant("finish_handle_facts", Observed::Faithful);
}

#[test]
fn selected_finish_leaves_a_top_handle_with_its_facts() {
    check_finish_invariant("finish_top_handle", Observed::Faithful);
}

#[test]
fn child_pop_return_has_the_child_return_facts() {
    check_finish_invariant("finish_child_return_facts", Observed::Faithful);
}

#[test]
fn scored_root_return_has_the_root_return_facts() {
    check_finish_invariant("finish_root_return_facts", Observed::Faithful);
}

#[test]
fn score_body_agreement_gives_the_handle_facts() {
    check_finish_invariant("handle_facts_from_agreement", Observed::Faithful);
}

#[test]
fn top_frame_agreement_gives_the_top_handle() {
    check_finish_invariant("top_handle_from_agreement", Observed::Faithful);
}

#[test]
fn attached_finish_handle_agrees_with_its_frame() {
    check_finish_invariant("attach_agreement_holds", Observed::Faithful);
}

#[test]
fn every_attach_of_a_nested_run_agrees_with_its_frame() {
    check_finish_invariant("attach_agreement_everywhere", Observed::Faithful);
}

#[test]
fn raw_par_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_raw_par", Observed::Faithful);
}

#[test]
fn cost_atom_absent_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_cost_atom_absent", Observed::Faithful);
}

#[test]
fn cost_atom_bytes_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_cost_atom_bytes", Observed::Faithful);
}

#[test]
fn cost_bound_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_cost_bound", Observed::Faithful);
}

#[test]
fn cost_atom_bound_forward_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_cost_atom_bound_forward", Observed::Faithful);
}

#[test]
fn cost_signature_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_cost_signature", Observed::Faithful);
}

#[test]
fn cost_region_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_cost_region", Observed::Faithful);
}

#[test]
fn cost_token_stack_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_cost_token_stack", Observed::Faithful);
}

#[test]
fn bundle_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_bundle", Observed::Faithful);
}

#[test]
fn if_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_if", Observed::Faithful);
}

#[test]
fn send_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_send", Observed::Faithful);
}

#[test]
fn match_component_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_match_component", Observed::Faithful);
}

#[test]
fn absent_specialized_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_absent_specialized", Observed::Faithful);
}

#[test]
fn absent_connective_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_absent_connective", Observed::Faithful);
}

#[test]
fn receive_bind_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_receive_bind", Observed::Faithful);
}

#[test]
fn match_case_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_match_case", Observed::Faithful);
}

#[test]
fn bool_oneof_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_bool_oneof", Observed::Faithful);
}

#[test]
fn expr_atom_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_expr_atom", Observed::Faithful);
}

#[test]
fn expr_numeric_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_expr_numeric", Observed::Faithful);
}

#[test]
fn expr_big_rational_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_expr_big_rational", Observed::Faithful);
}

#[test]
fn expr_fixed_point_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_expr_fixed_point", Observed::Faithful);
}

#[test]
fn sys_auth_token_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_sys_auth_token", Observed::Faithful);
}

#[test]
fn new_empty_collections_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_new_empty_collections", Observed::Faithful);
}

#[test]
fn new_one_injection_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_new_one_injection", Observed::Faithful);
}

#[test]
fn new_one_absent_injection_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_new_one_absent_injection", Observed::Faithful);
}

#[test]
fn new_one_uri_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_new_one_uri", Observed::Faithful);
}

#[test]
fn new_collections_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_new_collections", Observed::Faithful);
}

#[test]
fn expr_payload_forward_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_expr_payload_forward", Observed::Faithful);
}

#[test]
fn expr_structural_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_expr_structural", Observed::Faithful);
}

#[test]
fn evar_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_evar", Observed::Faithful);
}

#[test]
fn ordered_sequence_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_ordered_sequence", Observed::Faithful);
}

#[test]
fn emethod_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_emethod", Observed::Faithful);
}

#[test]
fn emap_finish_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_emap_finish", Observed::Faithful);
}

#[test]
fn connective_not_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_connective_not", Observed::Faithful);
}

#[test]
fn connective_varref_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_connective_varref", Observed::Faithful);
}

#[test]
fn connective_collection_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_connective_collection", Observed::Faithful);
}

#[test]
fn unforgeable_bytes_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_unforgeable_bytes", Observed::Faithful);
}

#[test]
fn receive_present_condition_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_receive_present_condition", Observed::Faithful);
}

#[test]
fn receive_absent_condition_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_receive_absent_condition", Observed::Faithful);
}

#[test]
fn nonsemantic_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_nonsemantic", Observed::Faithful);
}

#[test]
fn var_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_var", Observed::Faithful);
}

#[test]
fn epathmap_owner_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_epathmap_owner", Observed::Faithful);
}

#[test]
fn ezipper_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_ezipper", Observed::Faithful);
}

#[test]
fn finish_group_1_routes_have_the_top_handle_facts() {
    check_finish_invariant("handle_routed_group_1", Observed::Faithful);
}

#[test]
fn finish_group_2_routes_have_the_top_handle_facts() {
    check_finish_invariant("handle_routed_group_2", Observed::Faithful);
}

#[test]
fn finish_group_3_routes_have_the_top_handle_facts() {
    check_finish_invariant("handle_routed_group_3", Observed::Faithful);
}

#[test]
fn finish_group_4_routes_have_the_top_handle_facts() {
    check_finish_invariant("handle_routed_group_4", Observed::Faithful);
}

#[test]
fn finish_group_5_routes_have_the_top_handle_facts() {
    check_finish_invariant("handle_routed_group_5", Observed::Faithful);
}

#[test]
fn finish_group_6_routes_have_the_top_handle_facts() {
    check_finish_invariant("handle_routed_group_6", Observed::Faithful);
}

#[test]
fn every_selected_finish_route_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_selected_finish", Observed::Faithful);
}

#[test]
fn epathmap_source_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_epm_source_finish", Observed::Faithful);
}

#[test]
fn epathmap_product_finish_has_the_top_handle_facts() {
    check_finish_invariant("handle_after_product_epm_finish", Observed::Faithful);
}

#[test]
fn child_step_return_carries_the_child_handle() {
    check_finish_invariant("child_return_carries_handle", Observed::Faithful);
}

#[test]
fn pre_pop_child_accept_carries_the_child_handle() {
    check_finish_invariant("pre_pop_return_carries_handle", Observed::Faithful);
}

#[test]
fn scored_child_eof_accept_carries_the_child_handle() {
    check_finish_invariant("eof_score_child_carries_handle", Observed::Faithful);
}

#[test]
fn root_return_follows_from_the_top_handle() {
    check_finish_invariant("root_return_from_top_handle", Observed::Faithful);
}

#[test]
fn message_eof_outcomes_have_the_finish_facts() {
    check_finish_invariant("runtime_eof_finish_facts", Observed::Faithful);
}

#[test]
fn epathmap_eof_outcomes_have_the_finish_facts() {
    check_finish_invariant("runtime_epathmap_eof_finish_facts", Observed::Faithful);
}

#[test]
fn live_root_score_step_has_the_top_handle() {
    check_finish_invariant("live_root_score_has_top_handle", Observed::Faithful);
}

#[test]
fn live_child_score_step_has_the_top_handle() {
    check_finish_invariant("live_child_score_has_top_handle", Observed::Faithful);
}

#[test]
fn live_root_complete_carries_the_handle_to_the_return() {
    check_finish_invariant("live_root_complete_carries_handle", Observed::Faithful);
}

#[test]
fn nested_epathmap_owner_reaches_the_witness_finish() {
    check_finish_invariant("handle_owner_witness", Observed::Faithful);
}
