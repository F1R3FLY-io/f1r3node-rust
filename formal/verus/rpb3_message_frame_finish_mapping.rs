//! MessageFrame finish mapping over the generated descriptor table.
//!
//! Open obligation 1 of `docs/design/pathmap/rpb3-admission-source-traceability.md`
//! asks for a universal mapping from a production `MessageFrame` finish, which
//! is the selected `SemanticKind` together with its `reduce_semantic_kind`
//! branch, to a Verus prestate and a Verus finish branch.
//!
//! The finish branches are the branches of the runtime message EOF dispatch,
//! `runtime_message_driver_step(_, Eof | EofEPathMap)`:
//!
//! * the 43 candidates of `bridge_score_finish_selected`, in selection groups 1 to 6;
//! * the 15 arms of `runtime_noscore_child_kind`;
//! * the no-score root return `runtime_noscore_root_return`;
//! * the EPathMap snapshot finish `runtime_epm_source_score_finish_selected`.
//!
//! The integrated `RootScore` and `MessageChildScore` actions use the same
//! selector. `MessageChildComplete` and `RootComplete` are the later pop and
//! accept steps, and are not finish branches.
//!
//! The finish cases are computed from the generated table alone: a program
//! with a generated slot count, at the root, as the child of a frame_kind 1
//! descriptor row, or as the value of a frame_kind 2 row. The score outcome of
//! the selected reduce branch is either scored or unscored.
//!
//! The production half (`production_semantic_kind`,
//! `production_outcome_possible`) cannot be extracted from Rust inside Verus.
//! The kind table transcribes the `expected` array of the production test
//! `every_wire_type_id_resolves_to_its_expected_semantic_kind`. The outcome
//! table transcribes the score handling of each production reducer.
//!
//! Every route guard below is proved sound against the actual Verus step
//! definitions: if a step fires, its guard holds. The totality theorem is
//! therefore checked against the real finish branches, and its premises do not
//! assume a route exists.
//!
//! The 43 selector guards are proved one candidate at a time (`finish_guard_*`),
//! over the opaque candidate wrappers `finish_candidate_*` of
//! `rpb3_score_finish_groups`; a wrapper is its step's next state, so the
//! candidate exists exactly when the step fires. Each guard lemma reasons into
//! one step body, and the group lemmas take a fold apart with that module's
//! `finish_group_K_body`, so no query holds a step body it does not need.
//!
//! The mapping is total: every finish case of the generated table has an
//! admitted finish branch (`finish_mapping_case_total`,
//! `finish_mapping_total_universal`). Unscored roots finish through the
//! no-score root return. Var, the EPathMap owner and the scored EZipper finish
//! through selection group 6. The incomplete EMap child and the absent-PathMap
//! EZipper child finish through their no-score arms. Seven lemmas stated before
//! those branches existed, which described the cases without a branch, are
//! false or vacuous now; each is commented out with its reason. The six
//! group-wide selector guard lemmas are replaced by the per-candidate ones and
//! are commented out with their reason.
use vstd::prelude::*;

#[path = "rpb3_generated_descriptor_projection.rs"]
mod rpb3_generated_descriptor_projection;
use rpb3_generated_descriptor_projection as generated;
use crate::{
    rpb3_admission_cursor as admission, rpb3_finish_macro_refinement as finish,
    rpb3_message_driver_source_integration as driver,
    rpb3_runtime_frame_source_refinement as runtime, rpb3_score_admission_bridge as bridge,
    rpb3_score_finish_groups as groups,
};

verus! {

/// `canonical_semantics::ParComponentTag`; discriminants are `production_component_tag_byte`.
pub enum ProductionParComponentTag {
    Send,
    Receive,
    New,
    Expr,
    Match,
    GUnforgeable,
    Connective,
    Bundle,
    If,
    CostTokenStack,
}

pub open spec fn production_component_tag_byte(tag: ProductionParComponentTag) -> u32 {
    match tag {
        ProductionParComponentTag::Send => 1,
        ProductionParComponentTag::Receive => 2,
        ProductionParComponentTag::New => 4,
        ProductionParComponentTag::Expr => 5,
        ProductionParComponentTag::Match => 6,
        ProductionParComponentTag::GUnforgeable => 7,
        ProductionParComponentTag::Connective => 8,
        ProductionParComponentTag::Bundle => 11,
        ProductionParComponentTag::If => 12,
        ProductionParComponentTag::CostTokenStack => 13,
    }
}

pub open spec fn production_component_family(
    tag: ProductionParComponentTag,
) -> admission::AdmissionFamily {
    match tag {
        ProductionParComponentTag::Send => admission::AdmissionFamily::Send,
        ProductionParComponentTag::Receive => admission::AdmissionFamily::Receive,
        ProductionParComponentTag::New => admission::AdmissionFamily::New,
        ProductionParComponentTag::Expr => admission::AdmissionFamily::Expr,
        ProductionParComponentTag::Match => admission::AdmissionFamily::Match,
        ProductionParComponentTag::GUnforgeable => admission::AdmissionFamily::GUnforgeable,
        ProductionParComponentTag::Connective => admission::AdmissionFamily::Connective,
        ProductionParComponentTag::Bundle => admission::AdmissionFamily::Bundle,
        ProductionParComponentTag::If => admission::AdmissionFamily::If,
        ProductionParComponentTag::CostTokenStack => admission::AdmissionFamily::CostTokenStack,
    }
}

/// `protobuf_admission::SemanticKind`, one variant per `reduce_semantic_kind` arm.
pub enum ProductionSemanticKind {
    Opaque,
    RawPar,
    Component(ProductionParComponentTag),
    Var,
    Unit,
    ReceiveBind,
    CostSignature,
    CostSignatureAtom,
    CostBoundSignature,
    CostRegion,
    MatchCase,
    Expr,
    BigRational,
    FixedPoint,
    OrderedCollection { constructor: i32, has_remainder: bool, canonical_set: bool },
    EMap,
    EPathMap,
    EZipper,
    EMethod,
    KeyValuePair,
    ExprVar,
    ExprUnary(i32),
    ExprBinary(i32),
    Connective,
    VarRef,
    ConnectiveBody,
    GUnforgeable,
    BytesLeaf,
}

/// The `expected` array of the production test
/// `every_wire_type_id_resolves_to_its_expected_semantic_kind`, indexed by wire type ID.
pub open spec fn production_semantic_kind(type_id: u16) -> Option<ProductionSemanticKind> {
    match type_id {
        1 => Some(ProductionSemanticKind::RawPar),
        2 => Some(ProductionSemanticKind::Opaque),
        3 => Some(ProductionSemanticKind::Opaque),
        4 => Some(ProductionSemanticKind::Opaque),
        5 => Some(ProductionSemanticKind::Opaque),
        6 => Some(ProductionSemanticKind::Var),
        7 => Some(ProductionSemanticKind::Unit),
        8 => Some(ProductionSemanticKind::Component(ProductionParComponentTag::Bundle)),
        9 => Some(ProductionSemanticKind::CostSignature),
        10 => Some(ProductionSemanticKind::CostSignatureAtom),
        11 => Some(ProductionSemanticKind::CostBoundSignature),
        12 => Some(ProductionSemanticKind::CostRegion),
        13 => Some(ProductionSemanticKind::Component(ProductionParComponentTag::CostTokenStack)),
        14 => Some(ProductionSemanticKind::Opaque),
        15 => Some(ProductionSemanticKind::Component(ProductionParComponentTag::Send)),
        16 => Some(ProductionSemanticKind::ReceiveBind),
        17 => Some(ProductionSemanticKind::Opaque),
        18 => Some(ProductionSemanticKind::Opaque),
        19 => Some(ProductionSemanticKind::Component(ProductionParComponentTag::Receive)),
        20 => Some(ProductionSemanticKind::Component(ProductionParComponentTag::New)),
        21 => Some(ProductionSemanticKind::MatchCase),
        22 => Some(ProductionSemanticKind::Component(ProductionParComponentTag::Match)),
        23 => Some(ProductionSemanticKind::Component(ProductionParComponentTag::If)),
        24 => Some(ProductionSemanticKind::Expr),
        25 => Some(ProductionSemanticKind::BigRational),
        26 => Some(ProductionSemanticKind::FixedPoint),
        27 => Some(ProductionSemanticKind::OrderedCollection { constructor: 6, has_remainder: true, canonical_set: false }),
        28 => Some(ProductionSemanticKind::OrderedCollection { constructor: 7, has_remainder: false, canonical_set: false }),
        29 => Some(ProductionSemanticKind::OrderedCollection { constructor: 8, has_remainder: true, canonical_set: true }),
        30 => Some(ProductionSemanticKind::EMap),
        31 => Some(ProductionSemanticKind::EPathMap),
        32 => Some(ProductionSemanticKind::EZipper),
        33 => Some(ProductionSemanticKind::EMethod),
        34 => Some(ProductionSemanticKind::KeyValuePair),
        35 => Some(ProductionSemanticKind::ExprVar),
        36 => Some(ProductionSemanticKind::ExprUnary(112)),
        37 => Some(ProductionSemanticKind::ExprUnary(101)),
        38 => Some(ProductionSemanticKind::ExprBinary(102)),
        39 => Some(ProductionSemanticKind::ExprBinary(103)),
        40 => Some(ProductionSemanticKind::ExprBinary(122)),
        41 => Some(ProductionSemanticKind::ExprBinary(104)),
        42 => Some(ProductionSemanticKind::ExprBinary(105)),
        43 => Some(ProductionSemanticKind::ExprBinary(106)),
        44 => Some(ProductionSemanticKind::ExprBinary(107)),
        45 => Some(ProductionSemanticKind::ExprBinary(108)),
        46 => Some(ProductionSemanticKind::ExprBinary(109)),
        47 => Some(ProductionSemanticKind::ExprBinary(110)),
        48 => Some(ProductionSemanticKind::ExprBinary(111)),
        49 => Some(ProductionSemanticKind::ExprBinary(113)),
        50 => Some(ProductionSemanticKind::ExprBinary(114)),
        51 => Some(ProductionSemanticKind::ExprBinary(118)),
        52 => Some(ProductionSemanticKind::ExprBinary(119)),
        53 => Some(ProductionSemanticKind::ExprBinary(120)),
        54 => Some(ProductionSemanticKind::ExprBinary(121)),
        55 => Some(ProductionSemanticKind::Connective),
        56 => Some(ProductionSemanticKind::VarRef),
        57 => Some(ProductionSemanticKind::ConnectiveBody),
        58 => Some(ProductionSemanticKind::Opaque),
        59 => Some(ProductionSemanticKind::Opaque),
        60 => Some(ProductionSemanticKind::GUnforgeable),
        61 => Some(ProductionSemanticKind::BytesLeaf),
        62 => Some(ProductionSemanticKind::BytesLeaf),
        63 => Some(ProductionSemanticKind::BytesLeaf),
        64 => Some(ProductionSemanticKind::Unit),
        _ => None,
    }
}

/// Whether the reduce branch leaves a score handle in its returned summary.
pub enum ProductionScoreOutcome {
    Scored,
    Unscored,
}

/// Outcomes a successful production reducer can return. Transcribed from
/// `protobuf_admission.rs`: `reduce_semantic_kind` and the reducers it calls.
pub open spec fn production_outcome_possible(
    kind: ProductionSemanticKind, outcome: ProductionScoreOutcome,
) -> bool {
    match kind {
        ProductionSemanticKind::Opaque
        | ProductionSemanticKind::Unit
        | ProductionSemanticKind::BigRational
        | ProductionSemanticKind::FixedPoint
        | ProductionSemanticKind::KeyValuePair
        | ProductionSemanticKind::VarRef
        | ProductionSemanticKind::ConnectiveBody
        | ProductionSemanticKind::BytesLeaf => outcome == ProductionScoreOutcome::Unscored,
        ProductionSemanticKind::RawPar
        | ProductionSemanticKind::Var
        | ProductionSemanticKind::CostSignature
        | ProductionSemanticKind::CostSignatureAtom
        | ProductionSemanticKind::CostBoundSignature
        | ProductionSemanticKind::CostRegion
        | ProductionSemanticKind::Expr
        | ProductionSemanticKind::OrderedCollection { .. }
        | ProductionSemanticKind::EPathMap
        | ProductionSemanticKind::Connective
        | ProductionSemanticKind::GUnforgeable => outcome == ProductionScoreOutcome::Scored,
        ProductionSemanticKind::Component(tag) => match tag {
            ProductionParComponentTag::Expr
            | ProductionParComponentTag::Connective
            | ProductionParComponentTag::GUnforgeable => false,
            _ => outcome == ProductionScoreOutcome::Scored,
        },
        ProductionSemanticKind::ReceiveBind
        | ProductionSemanticKind::MatchCase
        | ProductionSemanticKind::EMap
        | ProductionSemanticKind::EZipper
        | ProductionSemanticKind::EMethod
        | ProductionSemanticKind::ExprVar
        | ProductionSemanticKind::ExprUnary(_)
        | ProductionSemanticKind::ExprBinary(_) => true,
    }
}

/// Admission family a production kind is driven under; compared with
/// `generated_message_family_for_type` in `finish_kind_table_matches_generated_table`.
pub open spec fn production_kind_family(kind: ProductionSemanticKind) -> admission::AdmissionFamily {
    match kind {
        ProductionSemanticKind::Component(tag) => production_component_family(tag),
        ProductionSemanticKind::Expr => admission::AdmissionFamily::Expr,
        ProductionSemanticKind::Connective => admission::AdmissionFamily::Connective,
        ProductionSemanticKind::GUnforgeable => admission::AdmissionFamily::GUnforgeable,
        ProductionSemanticKind::ReceiveBind => admission::AdmissionFamily::ReceiveBindFrame,
        ProductionSemanticKind::MatchCase => admission::AdmissionFamily::MatchCaseFrame,
        ProductionSemanticKind::CostSignature => admission::AdmissionFamily::CostSignatureFrame,
        ProductionSemanticKind::CostRegion => admission::AdmissionFamily::CostRegionFrame,
        ProductionSemanticKind::BigRational => admission::AdmissionFamily::BigRationalFrame,
        ProductionSemanticKind::FixedPoint => admission::AdmissionFamily::FixedPointFrame,
        ProductionSemanticKind::ExprVar => admission::AdmissionFamily::EVarFrame,
        _ => admission::AdmissionFamily::Other,
    }
}

pub enum MessageFramePosition {
    Root,
    Child { parent_program: u16, field_tag: u32 },
    MapValue { owner_program: u16, field_tag: u32 },
}

/// The program can be finished at the position according to the generated table.
pub open spec fn generated_message_position(
    program: u16, position: MessageFramePosition,
) -> bool {
    &&& generated::generated_slot_count(program).is_some()
    &&& match position {
        MessageFramePosition::Root => true,
        MessageFramePosition::Child { parent_program, field_tag } =>
            match generated::generated_descriptor(parent_program, field_tag) {
                Some(descriptor) => descriptor.frame_kind == 1
                    && descriptor.child_type_id == Some(program),
                None => false,
            },
        MessageFramePosition::MapValue { owner_program, field_tag } =>
            match generated::generated_descriptor(owner_program, field_tag) {
                Some(descriptor) => descriptor.frame_kind == 2
                    && descriptor.child_type_id == Some(program),
                None => false,
            },
    }
}

pub struct MessageFinishFrameShape {
    pub family: admission::AdmissionFamily,
    pub program_type_id: u16,
    pub frame_kind: u8,
    pub slot_count: usize,
}

pub struct MessageFinishParentShape {
    pub family: admission::AdmissionFamily,
    pub program_type_id: u16,
    pub frame_kind: u8,
    pub waiting_slot: Option<usize>,
    pub waiting_tag: Option<u32>,
}

pub enum MessageFinishPositionShape {
    Root,
    Child { parent: MessageFinishParentShape },
}

pub struct MessageFinishPrestate {
    pub frame: MessageFinishFrameShape,
    pub position: MessageFinishPositionShape,
}

/// The prestate at EOF, built from the generated table. The family is the
/// message driver's own assignment. A child's parent waits on the descriptor's
/// slot and tag. A map value's parent is the frame_kind 2 Map frame that
/// `admission_push_step` accepts for a map-value push.
pub open spec fn message_finish_prestate(
    program: u16, position: MessageFramePosition,
) -> Option<MessageFinishPrestate> {
    match generated::generated_slot_count(program) {
        None => None,
        Some(slot_count) => {
            let frame = MessageFinishFrameShape {
                family: driver::generated_message_family_for_type(program),
                program_type_id: program,
                frame_kind: 1,
                slot_count,
            };
            match position {
                MessageFramePosition::Root => Some(MessageFinishPrestate {
                    frame,
                    position: MessageFinishPositionShape::Root,
                }),
                MessageFramePosition::Child { parent_program, field_tag } =>
                    match generated::generated_descriptor(parent_program, field_tag) {
                        Some(descriptor) => Some(MessageFinishPrestate {
                            frame,
                            position: MessageFinishPositionShape::Child {
                                parent: MessageFinishParentShape {
                                    family: driver::generated_message_family_for_type(
                                        parent_program,
                                    ),
                                    program_type_id: parent_program,
                                    frame_kind: 1,
                                    waiting_slot: Some(descriptor.slot),
                                    waiting_tag: Some(field_tag),
                                },
                            },
                        }),
                        None => None,
                    },
                MessageFramePosition::MapValue { owner_program, field_tag } =>
                    Some(MessageFinishPrestate {
                        frame,
                        position: MessageFinishPositionShape::Child {
                            parent: MessageFinishParentShape {
                                family: admission::AdmissionFamily::Other,
                                program_type_id: 1,
                                frame_kind: 2,
                                waiting_slot: Some(1),
                                waiting_tag: Some(2),
                            },
                        },
                    }),
            }
        },
    }
}

pub open spec fn finish_frame_shape(frame: admission::AdmissionFrameState) -> MessageFinishFrameShape {
    MessageFinishFrameShape {
        family: frame.family,
        program_type_id: frame.program_type_id,
        frame_kind: frame.frame_kind,
        slot_count: frame.slot_count,
    }
}

pub open spec fn finish_parent_shape(frame: admission::AdmissionFrameState) -> MessageFinishParentShape {
    MessageFinishParentShape {
        family: frame.family,
        program_type_id: frame.program_type_id,
        frame_kind: frame.frame_kind,
        waiting_slot: frame.waiting_slot,
        waiting_tag: frame.waiting_tag,
    }
}

/// The shape of an actual Verus admission stack at EOF: its top frame and,
/// when there is one, the parent below it.
pub open spec fn finish_state_prestate(state: admission::AdmissionState) -> MessageFinishPrestate {
    MessageFinishPrestate {
        frame: finish_frame_shape(state.frames.last()),
        position: if state.frames.len() > 1 {
            MessageFinishPositionShape::Child {
                parent: finish_parent_shape(state.frames[state.frames.len() - 2]),
            }
        } else {
            MessageFinishPositionShape::Root
        },
    }
}

pub open spec fn finish_top_shape(before: bridge::ScoreAdmissionBridge) -> MessageFinishFrameShape {
    finish_frame_shape(before.admission.frames.last())
}

/// The Verus finish branches. The candidates of `bridge_score_finish_selected`
/// come first, by selection group: group 1 is `RawPar` to `CostTokenStack`,
/// group 2 `Bundle` to `MatchCase`, group 3 `BoolOneof` to `NewOneInjection`,
/// group 4 `NewOneAbsentInjection` to `EMapFinish`, group 5 `ConnectiveNot` to
/// `Nonsemantic` and group 6 `VarScore` to `EZipperScore`. Then the arms of
/// `runtime_noscore_child_kind` (`NoScoreReceiveBind` to `NoScoreEZipperChild`),
/// the no-score root return `runtime_noscore_root_return` (`NoScoreRoot`) and
/// the EPathMap snapshot finish `runtime_epm_source_score_finish_selected`
/// (`EPathMapSnapshot`).
pub enum MessageFinishRoute {
    RawPar,
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
    Match,
    AbsentSpecializedConstructor,
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
    EVar,
    OrderedSequence,
    EMethod,
    EMapFinish,
    ConnectiveNot,
    ConnectiveVarRef,
    ConnectiveCollection,
    UnforgeableBytes,
    ReceivePresentCondition,
    ReceiveAbsentCondition,
    Nonsemantic,
    VarScore,
    EPathMapOwnerScore,
    EZipperScore,
    NoScoreReceiveBind,
    NoScoreMatchCase,
    NoScoreBigRational,
    NoScoreFixedPoint,
    NoScoreUnit,
    NoScoreVarRef,
    NoScoreConnectiveBody,
    NoScoreBytes,
    NoScoreEVar,
    NoScoreEMethod,
    NoScoreExprStructural,
    NoScoreKeyValuePair,
    NoScoreOpaque,
    NoScoreEMapChild,
    NoScoreEZipperChild,
    NoScoreRoot,
    EPathMapSnapshot,
}

/// The arms of `runtime_noscore_child_kind`.
pub open spec fn message_finish_route_is_noscore_child(route: MessageFinishRoute) -> bool {
    match route {
        MessageFinishRoute::NoScoreReceiveBind
        | MessageFinishRoute::NoScoreMatchCase
        | MessageFinishRoute::NoScoreBigRational
        | MessageFinishRoute::NoScoreFixedPoint
        | MessageFinishRoute::NoScoreUnit
        | MessageFinishRoute::NoScoreVarRef
        | MessageFinishRoute::NoScoreConnectiveBody
        | MessageFinishRoute::NoScoreBytes
        | MessageFinishRoute::NoScoreEVar
        | MessageFinishRoute::NoScoreEMethod
        | MessageFinishRoute::NoScoreExprStructural
        | MessageFinishRoute::NoScoreKeyValuePair
        | MessageFinishRoute::NoScoreOpaque
        | MessageFinishRoute::NoScoreEMapChild
        | MessageFinishRoute::NoScoreEZipperChild => true,
        _ => false,
    }
}

/// The branches that finish without appending a score.
pub open spec fn message_finish_route_is_noscore(route: MessageFinishRoute) -> bool {
    message_finish_route_is_noscore_child(route) || route == MessageFinishRoute::NoScoreRoot
}

pub open spec fn message_finish_route_is_selector(route: MessageFinishRoute) -> bool {
    !message_finish_route_is_noscore(route) && route != MessageFinishRoute::EPathMapSnapshot
}

pub open spec fn message_finish_route_outcome(route: MessageFinishRoute) -> ProductionScoreOutcome {
    if message_finish_route_is_noscore(route) {
        ProductionScoreOutcome::Unscored
    } else {
        ProductionScoreOutcome::Scored
    }
}

/// The Other-family programs whose root `admission_root_derived_summary` derives.
pub open spec fn finish_noscore_root_other_program(program: u16) -> bool {
    match program {
        2 | 3 | 4 | 5 | 7 | 14 | 17 | 18 | 30 | 32 | 33 | 34 | 56 | 57 | 58 | 59 | 61 | 62
            | 63 | 64 => true,
        _ => 36 <= program && program <= 54,
    }
}

/// The root frames `runtime_noscore_root_return` can return: the families and
/// programs that `admission_root_derived_summary` derives a summary for.
pub open spec fn finish_noscore_root_frame(frame: MessageFinishFrameShape) -> bool {
    &&& frame.frame_kind == 1
    &&& match frame.family {
        admission::AdmissionFamily::ReceiveBindFrame => frame.program_type_id == 16,
        admission::AdmissionFamily::MatchCaseFrame => frame.program_type_id == 21,
        admission::AdmissionFamily::BigRationalFrame => frame.program_type_id == 25,
        admission::AdmissionFamily::FixedPointFrame => frame.program_type_id == 26,
        admission::AdmissionFamily::EVarFrame => frame.program_type_id == 35,
        admission::AdmissionFamily::Other => finish_noscore_root_other_program(
            frame.program_type_id,
        ),
        _ => false,
    }
}

/// Family, program, frame-kind and slot-count conditions that each finish
/// branch checks before it reads frame contents.
pub open spec fn message_finish_route_frame_guard(
    route: MessageFinishRoute, frame: MessageFinishFrameShape,
) -> bool {
    let family = frame.family;
    let program = frame.program_type_id;
    match route {
        MessageFinishRoute::RawPar => family == admission::AdmissionFamily::Other
            && program == 1 && frame.frame_kind == 1,
        MessageFinishRoute::CostAtomAbsent
        | MessageFinishRoute::CostAtomBytes
        | MessageFinishRoute::CostAtomBoundForward => program == 10,
        MessageFinishRoute::CostBound => program == 11,
        MessageFinishRoute::CostSignature =>
            family == admission::AdmissionFamily::CostSignatureFrame && program == 9,
        MessageFinishRoute::CostRegion =>
            family == admission::AdmissionFamily::CostRegionFrame && program == 12,
        MessageFinishRoute::CostTokenStack => family == admission::AdmissionFamily::CostTokenStack,
        MessageFinishRoute::Bundle => family == admission::AdmissionFamily::Bundle && program == 8,
        MessageFinishRoute::If => family == admission::AdmissionFamily::If && program == 23,
        MessageFinishRoute::Send => family == admission::AdmissionFamily::Send && program == 15,
        MessageFinishRoute::Match => family == admission::AdmissionFamily::Match && program == 22,
        MessageFinishRoute::AbsentSpecializedConstructor =>
            (family == admission::AdmissionFamily::Expr && program == 24)
            || (family == admission::AdmissionFamily::GUnforgeable && program == 60),
        MessageFinishRoute::AbsentConnective =>
            family == admission::AdmissionFamily::Connective && program == 55,
        MessageFinishRoute::ReceiveBind =>
            family == admission::AdmissionFamily::ReceiveBindFrame && program == 16,
        MessageFinishRoute::MatchCase =>
            family == admission::AdmissionFamily::MatchCaseFrame && program == 21,
        MessageFinishRoute::BoolOneof =>
            (family == admission::AdmissionFamily::Expr && program == 24)
            || (family == admission::AdmissionFamily::Connective && program == 55),
        MessageFinishRoute::ExprAtom
        | MessageFinishRoute::ExprNumeric
        | MessageFinishRoute::ExprBigRational
        | MessageFinishRoute::ExprFixedPoint
        | MessageFinishRoute::ExprPayloadForward =>
            family == admission::AdmissionFamily::Expr && program == 24,
        MessageFinishRoute::SysAuthToken
        | MessageFinishRoute::UnforgeableBytes =>
            family == admission::AdmissionFamily::GUnforgeable && program == 60,
        MessageFinishRoute::NewEmptyCollections
        | MessageFinishRoute::NewOneInjection
        | MessageFinishRoute::NewOneAbsentInjection
        | MessageFinishRoute::NewOneUri
        | MessageFinishRoute::NewCollections =>
            family == admission::AdmissionFamily::New && program == 20,
        MessageFinishRoute::ExprStructural => family == admission::AdmissionFamily::Other
            && frame.frame_kind == 1
            && bridge::bridge_expr_structural_constructor(program).is_some(),
        MessageFinishRoute::EVar => family == admission::AdmissionFamily::EVarFrame
            && program == 35 && frame.frame_kind == 1,
        MessageFinishRoute::OrderedSequence => family == admission::AdmissionFamily::Other
            && frame.frame_kind == 1
            && (program == 27 || program == 28 || program == 29)
            && frame.slot_count == (if program == 28 { 3usize } else { 4usize }),
        MessageFinishRoute::EMethod => family == admission::AdmissionFamily::Other
            && frame.frame_kind == 1 && program == 33,
        MessageFinishRoute::EMapFinish => family == admission::AdmissionFamily::Other
            && frame.frame_kind == 1 && program == 30,
        MessageFinishRoute::ConnectiveNot
        | MessageFinishRoute::ConnectiveVarRef
        | MessageFinishRoute::ConnectiveCollection =>
            family == admission::AdmissionFamily::Connective && program == 55,
        MessageFinishRoute::ReceivePresentCondition
        | MessageFinishRoute::ReceiveAbsentCondition =>
            family == admission::AdmissionFamily::Receive && program == 19,
        MessageFinishRoute::Nonsemantic => bridge::bridge_nonsemantic_family(family),
        MessageFinishRoute::VarScore => family == admission::AdmissionFamily::Other
            && program == 6 && frame.frame_kind == 1,
        MessageFinishRoute::EPathMapOwnerScore => family == admission::AdmissionFamily::Other
            && program == 31 && frame.frame_kind == 1,
        MessageFinishRoute::EZipperScore => family == admission::AdmissionFamily::Other
            && program == 32 && frame.frame_kind == 1 && frame.slot_count == 6,
        MessageFinishRoute::NoScoreReceiveBind => family
            == admission::AdmissionFamily::ReceiveBindFrame && program == 16
            && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreMatchCase => family
            == admission::AdmissionFamily::MatchCaseFrame && program == 21
            && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreBigRational => family
            == admission::AdmissionFamily::BigRationalFrame && program == 25
            && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreFixedPoint => family
            == admission::AdmissionFamily::FixedPointFrame && program == 26
            && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreUnit => family == admission::AdmissionFamily::Other
            && (program == 7 || program == 64) && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreVarRef => family == admission::AdmissionFamily::Other
            && program == 56 && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreConnectiveBody => family == admission::AdmissionFamily::Other
            && program == 57 && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreBytes => family == admission::AdmissionFamily::Other
            && (program == 61 || program == 62 || program == 63) && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreEVar => family == admission::AdmissionFamily::EVarFrame
            && program == 35 && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreEMethod => family == admission::AdmissionFamily::Other
            && program == 33 && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreExprStructural => family == admission::AdmissionFamily::Other
            && 36 <= program && program <= 54 && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreKeyValuePair => family == admission::AdmissionFamily::Other
            && program == 34 && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreOpaque => family == admission::AdmissionFamily::Other
            && (program == 3 || program == 14 || program == 17) && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreEMapChild => family == admission::AdmissionFamily::Other
            && program == 30 && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreEZipperChild => family == admission::AdmissionFamily::Other
            && program == 32 && frame.frame_kind == 1,
        MessageFinishRoute::NoScoreRoot => finish_noscore_root_frame(frame),
        MessageFinishRoute::EPathMapSnapshot => family == admission::AdmissionFamily::Other
            && program == 31 && frame.frame_kind == 1,
    }
}

/// The edge table of `admission_opaque_child_edge`.
pub open spec fn finish_opaque_edge(
    parent_program: u16, waiting_slot: Option<usize>, waiting_tag: Option<u32>,
    child_program: u16,
) -> bool {
    match (parent_program, waiting_slot, waiting_tag, child_program) {
        (2, Some(0), Some(1), 3) | (2, Some(3), Some(5), 14) | (18, Some(0), Some(1), 17) => true,
        _ => false,
    }
}

/// No-score child arms need a parent (`frames.len() > 1`). Seven of them also
/// check the parent itself. The no-score root return needs the root position.
/// Selector and EPathMap branches do not depend on position.
pub open spec fn message_finish_route_position_guard(
    route: MessageFinishRoute, frame: MessageFinishFrameShape,
    position: MessageFinishPositionShape,
) -> bool {
    if route == MessageFinishRoute::NoScoreRoot {
        match position {
            MessageFinishPositionShape::Root => true,
            MessageFinishPositionShape::Child { .. } => false,
        }
    } else if !message_finish_route_is_noscore_child(route) {
        true
    } else {
        match position {
            MessageFinishPositionShape::Root => false,
            MessageFinishPositionShape::Child { parent } => match route {
                MessageFinishRoute::NoScoreEVar =>
                    parent.family == admission::AdmissionFamily::Expr
                        && parent.program_type_id == 24
                        && parent.waiting_slot == Some(0usize)
                        && parent.waiting_tag == Some(19u32),
                MessageFinishRoute::NoScoreEMethod =>
                    parent.family == admission::AdmissionFamily::Expr
                        && parent.program_type_id == 24
                        && parent.waiting_slot == Some(0usize)
                        && parent.waiting_tag == Some(24u32),
                MessageFinishRoute::NoScoreExprStructural =>
                    parent.family == admission::AdmissionFamily::Expr
                        && parent.program_type_id == 24
                        && parent.waiting_slot == Some(0usize),
                MessageFinishRoute::NoScoreKeyValuePair =>
                    parent.family == admission::AdmissionFamily::Other
                        && parent.program_type_id == 30
                        && parent.frame_kind == 1
                        && parent.waiting_slot == Some(0usize)
                        && parent.waiting_tag == Some(1u32),
                MessageFinishRoute::NoScoreOpaque =>
                    parent.family == admission::AdmissionFamily::Other
                        && parent.frame_kind == 1
                        && finish_opaque_edge(
                            parent.program_type_id, parent.waiting_slot, parent.waiting_tag,
                            frame.program_type_id,
                        ),
                MessageFinishRoute::NoScoreEMapChild =>
                    parent.family == admission::AdmissionFamily::Expr
                        && parent.program_type_id == 24
                        && parent.waiting_slot == Some(0usize)
                        && parent.waiting_tag == Some(23u32),
                MessageFinishRoute::NoScoreEZipperChild =>
                    parent.family == admission::AdmissionFamily::Expr
                        && parent.program_type_id == 24
                        && parent.waiting_slot == Some(0usize)
                        && parent.waiting_tag == Some(33u32),
                _ => true,
            },
        }
    }
}

pub open spec fn message_finish_route_guard(
    route: MessageFinishRoute, prestate: MessageFinishPrestate,
) -> bool {
    &&& message_finish_route_frame_guard(route, prestate.frame)
    &&& message_finish_route_position_guard(route, prestate.frame, prestate.position)
}

/// The no-score child arm selected by the top frame's family and program.
pub open spec fn finish_noscore_route_for(frame: MessageFinishFrameShape) -> MessageFinishRoute {
    let program = frame.program_type_id;
    match frame.family {
        admission::AdmissionFamily::ReceiveBindFrame => MessageFinishRoute::NoScoreReceiveBind,
        admission::AdmissionFamily::MatchCaseFrame => MessageFinishRoute::NoScoreMatchCase,
        admission::AdmissionFamily::BigRationalFrame => MessageFinishRoute::NoScoreBigRational,
        admission::AdmissionFamily::FixedPointFrame => MessageFinishRoute::NoScoreFixedPoint,
        admission::AdmissionFamily::EVarFrame => MessageFinishRoute::NoScoreEVar,
        _ => if program == 7 || program == 64 {
            MessageFinishRoute::NoScoreUnit
        } else if program == 56 {
            MessageFinishRoute::NoScoreVarRef
        } else if program == 57 {
            MessageFinishRoute::NoScoreConnectiveBody
        } else if program == 61 || program == 62 || program == 63 {
            MessageFinishRoute::NoScoreBytes
        } else if program == 33 {
            MessageFinishRoute::NoScoreEMethod
        } else if program == 34 {
            MessageFinishRoute::NoScoreKeyValuePair
        } else if 36 <= program && program <= 54 {
            MessageFinishRoute::NoScoreExprStructural
        } else if program == 30 {
            MessageFinishRoute::NoScoreEMapChild
        } else if program == 32 {
            MessageFinishRoute::NoScoreEZipperChild
        } else {
            MessageFinishRoute::NoScoreOpaque
        },
    }
}

/// The Verus finish branches that implement a production reduce arm. Every
/// kind with an unscored outcome also finishes at the root through
/// `NoScoreRoot`.
pub open spec fn production_kind_route(
    kind: ProductionSemanticKind, route: MessageFinishRoute,
) -> bool {
    match kind {
        ProductionSemanticKind::RawPar => route == MessageFinishRoute::RawPar,
        ProductionSemanticKind::Component(tag) => match tag {
            ProductionParComponentTag::Send => route == MessageFinishRoute::Send
                || route == MessageFinishRoute::Nonsemantic,
            ProductionParComponentTag::Receive => route
                == MessageFinishRoute::ReceivePresentCondition
                || route == MessageFinishRoute::ReceiveAbsentCondition
                || route == MessageFinishRoute::Nonsemantic,
            ProductionParComponentTag::New => route == MessageFinishRoute::NewEmptyCollections
                || route == MessageFinishRoute::NewOneInjection
                || route == MessageFinishRoute::NewOneAbsentInjection
                || route == MessageFinishRoute::NewOneUri
                || route == MessageFinishRoute::NewCollections
                || route == MessageFinishRoute::Nonsemantic,
            ProductionParComponentTag::Match => route == MessageFinishRoute::Match
                || route == MessageFinishRoute::Nonsemantic,
            ProductionParComponentTag::Bundle => route == MessageFinishRoute::Bundle
                || route == MessageFinishRoute::Nonsemantic,
            ProductionParComponentTag::If => route == MessageFinishRoute::If
                || route == MessageFinishRoute::Nonsemantic,
            ProductionParComponentTag::CostTokenStack => route
                == MessageFinishRoute::CostTokenStack,
            ProductionParComponentTag::Expr
            | ProductionParComponentTag::Connective
            | ProductionParComponentTag::GUnforgeable => false,
        },
        ProductionSemanticKind::Var => route == MessageFinishRoute::VarScore,
        ProductionSemanticKind::Unit => route == MessageFinishRoute::NoScoreUnit
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::ReceiveBind => route == MessageFinishRoute::ReceiveBind
            || route == MessageFinishRoute::NoScoreReceiveBind
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::CostSignature => route == MessageFinishRoute::CostSignature,
        ProductionSemanticKind::CostSignatureAtom => route == MessageFinishRoute::CostAtomAbsent
            || route == MessageFinishRoute::CostAtomBytes
            || route == MessageFinishRoute::CostAtomBoundForward,
        ProductionSemanticKind::CostBoundSignature => route == MessageFinishRoute::CostBound,
        ProductionSemanticKind::CostRegion => route == MessageFinishRoute::CostRegion,
        ProductionSemanticKind::MatchCase => route == MessageFinishRoute::MatchCase
            || route == MessageFinishRoute::NoScoreMatchCase
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::Expr => route == MessageFinishRoute::AbsentSpecializedConstructor
            || route == MessageFinishRoute::BoolOneof
            || route == MessageFinishRoute::ExprAtom
            || route == MessageFinishRoute::ExprNumeric
            || route == MessageFinishRoute::ExprBigRational
            || route == MessageFinishRoute::ExprFixedPoint
            || route == MessageFinishRoute::ExprPayloadForward
            || route == MessageFinishRoute::Nonsemantic,
        ProductionSemanticKind::BigRational => route == MessageFinishRoute::NoScoreBigRational
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::FixedPoint => route == MessageFinishRoute::NoScoreFixedPoint
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::OrderedCollection { .. } => route
            == MessageFinishRoute::OrderedSequence,
        ProductionSemanticKind::EMap => route == MessageFinishRoute::EMapFinish
            || route == MessageFinishRoute::NoScoreEMapChild
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::EPathMap => route == MessageFinishRoute::EPathMapSnapshot
            || route == MessageFinishRoute::EPathMapOwnerScore,
        ProductionSemanticKind::EZipper => route == MessageFinishRoute::EZipperScore
            || route == MessageFinishRoute::NoScoreEZipperChild
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::EMethod => route == MessageFinishRoute::EMethod
            || route == MessageFinishRoute::NoScoreEMethod
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::KeyValuePair => route == MessageFinishRoute::NoScoreKeyValuePair
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::ExprVar => route == MessageFinishRoute::EVar
            || route == MessageFinishRoute::NoScoreEVar
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::ExprUnary(_)
        | ProductionSemanticKind::ExprBinary(_) => route == MessageFinishRoute::ExprStructural
            || route == MessageFinishRoute::NoScoreExprStructural
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::Connective => route == MessageFinishRoute::AbsentConnective
            || route == MessageFinishRoute::BoolOneof
            || route == MessageFinishRoute::ConnectiveNot
            || route == MessageFinishRoute::ConnectiveVarRef
            || route == MessageFinishRoute::ConnectiveCollection,
        ProductionSemanticKind::VarRef => route == MessageFinishRoute::NoScoreVarRef
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::ConnectiveBody => route
            == MessageFinishRoute::NoScoreConnectiveBody
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::GUnforgeable => route
            == MessageFinishRoute::AbsentSpecializedConstructor
            || route == MessageFinishRoute::SysAuthToken
            || route == MessageFinishRoute::UnforgeableBytes,
        ProductionSemanticKind::BytesLeaf => route == MessageFinishRoute::NoScoreBytes
            || route == MessageFinishRoute::NoScoreRoot,
        ProductionSemanticKind::Opaque => route == MessageFinishRoute::NoScoreOpaque
            || route == MessageFinishRoute::NoScoreRoot,
    }
}

/// The finish relation. A route is admitted for a finish case when it
/// implements the kind's reduce arm, has the case's score class, and its guard
/// accepts the mapped prestate.
pub open spec fn message_finish_route_admitted(
    program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition,
    route: MessageFinishRoute,
) -> bool {
    match (production_semantic_kind(program), message_finish_prestate(program, position)) {
        (Some(kind), Some(prestate)) => {
            &&& production_outcome_possible(kind, outcome)
            &&& production_kind_route(kind, route)
            &&& message_finish_route_outcome(route) == outcome
            &&& message_finish_route_guard(route, prestate)
        },
        _ => false,
    }
}

/// One representative branch per (kind, outcome, position class); the totality
/// witness. An unscored root finishes through `NoScoreRoot`; an unscored child
/// through its no-score child arm.
pub open spec fn message_finish_route_choice(
    program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition,
) -> Option<MessageFinishRoute> {
    match production_semantic_kind(program) {
        None => None,
        Some(kind) => match outcome {
            ProductionScoreOutcome::Scored => match kind {
                ProductionSemanticKind::RawPar => Some(MessageFinishRoute::RawPar),
                ProductionSemanticKind::Component(tag) => match tag {
                    ProductionParComponentTag::Send => Some(MessageFinishRoute::Send),
                    ProductionParComponentTag::Receive => Some(
                        MessageFinishRoute::ReceivePresentCondition,
                    ),
                    ProductionParComponentTag::New => Some(MessageFinishRoute::NewCollections),
                    ProductionParComponentTag::Match => Some(MessageFinishRoute::Match),
                    ProductionParComponentTag::Bundle => Some(MessageFinishRoute::Bundle),
                    ProductionParComponentTag::If => Some(MessageFinishRoute::If),
                    ProductionParComponentTag::CostTokenStack => Some(
                        MessageFinishRoute::CostTokenStack,
                    ),
                    _ => None,
                },
                ProductionSemanticKind::Var => Some(MessageFinishRoute::VarScore),
                ProductionSemanticKind::ReceiveBind => Some(MessageFinishRoute::ReceiveBind),
                ProductionSemanticKind::CostSignature => Some(MessageFinishRoute::CostSignature),
                ProductionSemanticKind::CostSignatureAtom => Some(
                    MessageFinishRoute::CostAtomBytes,
                ),
                ProductionSemanticKind::CostBoundSignature => Some(MessageFinishRoute::CostBound),
                ProductionSemanticKind::CostRegion => Some(MessageFinishRoute::CostRegion),
                ProductionSemanticKind::MatchCase => Some(MessageFinishRoute::MatchCase),
                ProductionSemanticKind::Expr => Some(MessageFinishRoute::ExprPayloadForward),
                ProductionSemanticKind::OrderedCollection { .. } => Some(
                    MessageFinishRoute::OrderedSequence,
                ),
                ProductionSemanticKind::EMap => Some(MessageFinishRoute::EMapFinish),
                ProductionSemanticKind::EPathMap => Some(MessageFinishRoute::EPathMapSnapshot),
                ProductionSemanticKind::EZipper => Some(MessageFinishRoute::EZipperScore),
                ProductionSemanticKind::EMethod => Some(MessageFinishRoute::EMethod),
                ProductionSemanticKind::ExprVar => Some(MessageFinishRoute::EVar),
                ProductionSemanticKind::ExprUnary(_)
                | ProductionSemanticKind::ExprBinary(_) => Some(
                    MessageFinishRoute::ExprStructural,
                ),
                ProductionSemanticKind::Connective => Some(
                    MessageFinishRoute::ConnectiveCollection,
                ),
                ProductionSemanticKind::GUnforgeable => Some(
                    MessageFinishRoute::UnforgeableBytes,
                ),
                _ => None,
            },
            ProductionScoreOutcome::Unscored => match position {
                MessageFramePosition::Root => Some(MessageFinishRoute::NoScoreRoot),
                _ => match kind {
                    ProductionSemanticKind::Unit => Some(MessageFinishRoute::NoScoreUnit),
                    ProductionSemanticKind::ReceiveBind => Some(
                        MessageFinishRoute::NoScoreReceiveBind,
                    ),
                    ProductionSemanticKind::MatchCase => Some(
                        MessageFinishRoute::NoScoreMatchCase,
                    ),
                    ProductionSemanticKind::BigRational => Some(
                        MessageFinishRoute::NoScoreBigRational,
                    ),
                    ProductionSemanticKind::FixedPoint => Some(
                        MessageFinishRoute::NoScoreFixedPoint,
                    ),
                    ProductionSemanticKind::EMap => Some(MessageFinishRoute::NoScoreEMapChild),
                    ProductionSemanticKind::EZipper => Some(
                        MessageFinishRoute::NoScoreEZipperChild,
                    ),
                    ProductionSemanticKind::EMethod => Some(MessageFinishRoute::NoScoreEMethod),
                    ProductionSemanticKind::KeyValuePair => Some(
                        MessageFinishRoute::NoScoreKeyValuePair,
                    ),
                    ProductionSemanticKind::ExprVar => Some(MessageFinishRoute::NoScoreEVar),
                    ProductionSemanticKind::ExprUnary(_)
                    | ProductionSemanticKind::ExprBinary(_) => Some(
                        MessageFinishRoute::NoScoreExprStructural,
                    ),
                    ProductionSemanticKind::VarRef => Some(MessageFinishRoute::NoScoreVarRef),
                    ProductionSemanticKind::ConnectiveBody => Some(
                        MessageFinishRoute::NoScoreConnectiveBody,
                    ),
                    ProductionSemanticKind::BytesLeaf => Some(MessageFinishRoute::NoScoreBytes),
                    ProductionSemanticKind::Opaque => Some(MessageFinishRoute::NoScoreOpaque),
                    _ => None,
                },
            },
        },
    }
}

/// The mapping: the representative branch, provided the relation admits it.
pub open spec fn message_finish_mapping(
    program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition,
) -> Option<MessageFinishRoute> {
    match message_finish_route_choice(program, outcome, position) {
        Some(route) => if message_finish_route_admitted(program, outcome, position, route) {
            Some(route)
        } else {
            None
        },
        None => None,
    }
}

/// A finish case of the generated table.
pub open spec fn message_finish_case(
    program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition,
) -> bool {
    &&& generated_message_position(program, position)
    &&& match production_semantic_kind(program) {
        Some(kind) => production_outcome_possible(kind, outcome),
        None => true,
    }
}

// Disabled: the gap is empty; every finish case has an admitted branch (finish_mapping_case_total).
// /// The exact finish cases with no Verus finish branch in the integrated crate.
// pub open spec fn finish_mapping_gap(
//     program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition,
// ) -> bool {
//     ||| (outcome == ProductionScoreOutcome::Unscored && position == MessageFramePosition::Root)
//     ||| program == 6
//     ||| program == 32
//     ||| (program == 30 && outcome == ProductionScoreOutcome::Unscored)
// }

// Disabled: empty; every table program has a finish branch at the root (finish_mapping_root_total).
// /// Table programs whose message frame has no finish branch at all.
// pub open spec fn finish_no_route_program(program: u16) -> bool {
//     program == 2 || program == 4 || program == 5 || program == 6 || program == 18
//         || program == 32 || program == 58 || program == 59
// }

pub open spec fn finish_frame_has_selector_route(frame: MessageFinishFrameShape) -> bool {
    exists|route: MessageFinishRoute|
        message_finish_route_is_selector(route)
            && #[trigger] message_finish_route_frame_guard(route, frame)
}

pub proof fn finish_kind_table_matches_generated_table(program: u16)
    ensures
        production_semantic_kind(program).is_some()
            == generated::generated_slot_count(program).is_some(),
        generated::generated_slot_count(program).is_some() ==> {
            &&& production_outcome_possible(
                production_semantic_kind(program).unwrap(), ProductionScoreOutcome::Scored,
            ) || production_outcome_possible(
                production_semantic_kind(program).unwrap(), ProductionScoreOutcome::Unscored,
            )
            &&& production_kind_family(production_semantic_kind(program).unwrap())
                == driver::generated_message_family_for_type(program)
        },
{
}

pub proof fn finish_component_tags_agree(tag: ProductionParComponentTag)
    ensures
        finish::component_tag_for_family(production_component_family(tag))
            == Some(production_component_tag_byte(tag) as u8),
        match generated::generated_descriptor(1, production_component_tag_byte(tag)) {
            Some(descriptor) => {
                &&& descriptor.frame_kind == 1
                &&& descriptor.child_type_id
                    == admission::admission_family_program_id(production_component_family(tag))
                &&& driver::generated_message_family_for_type(descriptor.child_type_id.unwrap())
                    == production_component_family(tag)
            },
            None => false,
        },
        admission::admission_raw_par_component_child(
            production_component_tag_byte(tag),
            admission::admission_family_witness_frame(
                production_component_family(tag), Set::empty(), Seq::empty(),
            ),
        ),
{
}

pub proof fn finish_production_constants_agree(program: u16)
    ensures
        match production_semantic_kind(program) {
            Some(ProductionSemanticKind::ExprUnary(constructor)) =>
                bridge::bridge_expr_structural_constructor(program) == Some((constructor, false)),
            Some(ProductionSemanticKind::ExprBinary(constructor)) =>
                bridge::bridge_expr_structural_constructor(program) == Some((constructor, true)),
            Some(ProductionSemanticKind::OrderedCollection {
                constructor, has_remainder, canonical_set,
            }) => {
                &&& constructor == (if program == 27 { 6i32 } else if program == 28 { 7i32 }
                    else { 8i32 })
                &&& has_remainder == (generated::generated_slot_count(program) == Some(4usize))
                &&& canonical_set == (program == 29)
            },
            _ => bridge::bridge_expr_structural_constructor(program).is_none(),
        },
{
}

pub proof fn finish_prestate_agrees_with_admission(
    program: u16, position: MessageFramePosition,
)
    requires generated_message_position(program, position),
    ensures
        message_finish_prestate(program, position).is_some(),
        message_finish_prestate(program, position).unwrap().frame.family
            == driver::generated_message_family_for_type(program),
        match admission::admission_family_program_id(
            message_finish_prestate(program, position).unwrap().frame.family,
        ) {
            Some(id) => id == program,
            None => true,
        },
        match admission::admission_family_slot_count(
            message_finish_prestate(program, position).unwrap().frame.family,
        ) {
            Some(count) => generated::generated_slot_count(program) == Some(count),
            None => true,
        },
        match position {
            MessageFramePosition::Child { parent_program, field_tag } => match (
                generated::generated_descriptor(parent_program, field_tag),
                generated::generated_slot_count(parent_program),
            ) {
                (Some(descriptor), Some(parent_slots)) => descriptor.slot < parent_slots,
                _ => false,
            },
            _ => true,
        },
{
}

pub proof fn finish_descriptor_constrained_parents(parent_program: u16, field_tag: u32)
    ensures
        match generated::generated_descriptor(parent_program, field_tag) {
            Some(descriptor) => descriptor.frame_kind == 1 ==> match descriptor.child_type_id {
                Some(child) => {
                    &&& child == 35 ==> parent_program == 24 && field_tag == 19
                        && descriptor.slot == 0
                    &&& child == 33 ==> parent_program == 24 && field_tag == 24
                        && descriptor.slot == 0
                    &&& (36 <= child && child <= 54) ==> parent_program == 24
                        && descriptor.slot == 0
                    &&& child == 34 ==> parent_program == 30 && field_tag == 1
                        && descriptor.slot == 0
                    &&& child == 30 ==> parent_program == 24 && field_tag == 23
                        && descriptor.slot == 0
                    &&& child == 32 ==> parent_program == 24 && field_tag == 33
                        && descriptor.slot == 0
                    &&& (child == 3 || child == 14 || child == 17) ==> finish_opaque_edge(
                        parent_program, Some(descriptor.slot), Some(field_tag), child,
                    )
                },
                None => false,
            },
            None => true,
        },
{
}

pub proof fn finish_mapping_root_total(program: u16, outcome: ProductionScoreOutcome)
    requires generated_message_position(program, MessageFramePosition::Root),
    ensures
        production_semantic_kind(program).is_some(),
        production_outcome_possible(production_semantic_kind(program).unwrap(), outcome)
            ==> message_finish_mapping(program, outcome, MessageFramePosition::Root).is_some(),
{
}

pub proof fn finish_mapping_child_total(
    program: u16, outcome: ProductionScoreOutcome, parent_program: u16, field_tag: u32,
)
    requires
        generated_message_position(
            program, MessageFramePosition::Child { parent_program, field_tag },
        ),
    ensures
        production_semantic_kind(program).is_some(),
        production_outcome_possible(production_semantic_kind(program).unwrap(), outcome)
            ==> message_finish_mapping(
                program, outcome, MessageFramePosition::Child { parent_program, field_tag },
            ).is_some(),
{
    finish_descriptor_constrained_parents(parent_program, field_tag);
}

pub proof fn finish_mapping_map_value_total(
    program: u16, outcome: ProductionScoreOutcome, owner_program: u16, field_tag: u32,
)
    requires
        generated_message_position(
            program, MessageFramePosition::MapValue { owner_program, field_tag },
        ),
    ensures
        program == 1,
        production_semantic_kind(program).is_some(),
        production_outcome_possible(production_semantic_kind(program).unwrap(), outcome)
            ==> message_finish_mapping(
                program, outcome, MessageFramePosition::MapValue { owner_program, field_tag },
            ).is_some(),
{
}

/// Totality over the generated descriptor table: every finish case has an
/// admitted Verus finish branch, and the mapping picks one.
pub proof fn finish_mapping_case_total(
    program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition,
)
    requires message_finish_case(program, outcome, position),
    ensures
        production_semantic_kind(program).is_some(),
        message_finish_mapping(program, outcome, position).is_some(),
        message_finish_route_admitted(
            program, outcome, position, message_finish_mapping(program, outcome, position).unwrap(),
        ),
{
    match position {
        MessageFramePosition::Root => finish_mapping_root_total(program, outcome),
        MessageFramePosition::Child { parent_program, field_tag } =>
            finish_mapping_child_total(program, outcome, parent_program, field_tag),
        MessageFramePosition::MapValue { owner_program, field_tag } =>
            finish_mapping_map_value_total(program, outcome, owner_program, field_tag),
    }
}

/// The universal statement over the generated descriptor table, closed form.
pub proof fn finish_mapping_total_universal()
    ensures
        forall|program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition|
            #[trigger] message_finish_case(program, outcome, position) ==> {
                &&& message_finish_mapping(program, outcome, position).is_some()
                &&& message_finish_route_admitted(
                    program, outcome, position,
                    message_finish_mapping(program, outcome, position).unwrap(),
                )
            },
{
    assert forall|program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition|
        #[trigger] message_finish_case(program, outcome, position) implies {
            &&& message_finish_mapping(program, outcome, position).is_some()
            &&& message_finish_route_admitted(
                program, outcome, position,
                message_finish_mapping(program, outcome, position).unwrap(),
            )
        } by {
        finish_mapping_case_total(program, outcome, position);
    }
}

// Disabled: vacuous; the gap it quantified over is empty (finish_mapping_case_total).
// pub proof fn finish_mapping_gap_has_no_route(
//     program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition,
//     route: MessageFinishRoute,
// )
//     requires
//         generated_message_position(program, position),
//         finish_mapping_gap(program, outcome, position),
//     ensures
//         !(message_finish_route_outcome(route) == outcome && message_finish_route_guard(
//             route, message_finish_prestate(program, position).unwrap(),
//         )),
//         !message_finish_route_admitted(program, outcome, position, route),
//         message_finish_mapping(program, outcome, position).is_none(),
// {
// }

// Disabled: false; the Var child (6, Scored, 16/3) is admitted by VarScore (finish_mapping_total_universal).
// pub proof fn finish_mapping_universal_totality_refuted()
//     ensures
//         message_finish_case(
//             6, ProductionScoreOutcome::Scored,
//             MessageFramePosition::Child { parent_program: 16, field_tag: 3 },
//         ),
//         forall|route: MessageFinishRoute| !#[trigger] message_finish_route_admitted(
//             6, ProductionScoreOutcome::Scored,
//             MessageFramePosition::Child { parent_program: 16, field_tag: 3 }, route,
//         ),
//         !(forall|program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition|
//             #[trigger] message_finish_case(program, outcome, position)
//                 ==> message_finish_mapping(program, outcome, position).is_some()),
// {
//     let position = MessageFramePosition::Child { parent_program: 16, field_tag: 3 };
//     assert(message_finish_case(6, ProductionScoreOutcome::Scored, position));
//     assert(message_finish_mapping(6, ProductionScoreOutcome::Scored, position).is_none());
//     assert forall|route: MessageFinishRoute| !#[trigger] message_finish_route_admitted(
//         6, ProductionScoreOutcome::Scored, position, route,
//     ) by {
//         finish_mapping_gap_has_no_route(6, ProductionScoreOutcome::Scored, position, route);
//     }
// }

// Disabled: stated over the removed gap; superseded by finish_mapping_case_total.
// /// The gap is exact: a finish case of the generated table has a Verus finish
// /// branch if and only if it is off the gap.
// pub proof fn finish_mapping_gap_exact(
//     program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition,
// )
//     requires message_finish_case(program, outcome, position),
//     ensures
//         message_finish_mapping(program, outcome, position).is_some()
//             == !finish_mapping_gap(program, outcome, position),
//         (exists|route: MessageFinishRoute|
//             #[trigger] message_finish_route_admitted(program, outcome, position, route))
//             == !finish_mapping_gap(program, outcome, position),
// {
//     finish_mapping_total_off_gap(program, outcome, position);
//     if finish_mapping_gap(program, outcome, position) {
//         assert forall|route: MessageFinishRoute|
//             !#[trigger] message_finish_route_admitted(program, outcome, position, route) by {
//             finish_mapping_gap_has_no_route(program, outcome, position, route);
//         }
//     } else {
//         let route = message_finish_mapping(program, outcome, position).unwrap();
//         assert(message_finish_route_admitted(program, outcome, position, route));
//     }
// }

// Disabled: stated over the removed gap; superseded by finish_mapping_total_universal.
// /// The universal statement over the generated descriptor table, closed form.
// pub proof fn finish_mapping_universal_off_gap()
//     ensures
//         forall|program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition|
//             #[trigger] message_finish_case(program, outcome, position)
//                 ==> (message_finish_mapping(program, outcome, position).is_some()
//                     == !finish_mapping_gap(program, outcome, position)),
// {
//     assert forall|program: u16, outcome: ProductionScoreOutcome, position: MessageFramePosition|
//         #[trigger] message_finish_case(program, outcome, position)
//             implies (message_finish_mapping(program, outcome, position).is_some()
//                 == !finish_mapping_gap(program, outcome, position)) by {
//         finish_mapping_gap_exact(program, outcome, position);
//     }
// }

pub proof fn finish_mapping_routes_complete(
    program: u16, position: MessageFramePosition, route: MessageFinishRoute,
)
    requires
        generated_message_position(program, position),
        message_finish_route_guard(route, message_finish_prestate(program, position).unwrap()),
    ensures
        production_kind_route(production_semantic_kind(program).unwrap(), route),
        production_outcome_possible(
            production_semantic_kind(program).unwrap(), message_finish_route_outcome(route),
        ),
        message_finish_route_admitted(
            program, message_finish_route_outcome(route), position, route,
        ),
{
}

// Disabled: replaced by the per-candidate finish_guard_* lemmas, one step body per query; a whole group in one query took up to 27,178,823 units (group 4).
// pub proof fn finish_selector_group_1_guard_sound(before: bridge::ScoreAdmissionBridge)
//     ensures
//         bridge::bridge_raw_par_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::RawPar, finish_top_shape(before),
//         ),
//         bridge::bridge_cost_atom_absent_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::CostAtomAbsent, finish_top_shape(before),
//         ),
//         bridge::bridge_cost_atom_bytes_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::CostAtomBytes, finish_top_shape(before),
//         ),
//         bridge::bridge_cost_bound_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::CostBound, finish_top_shape(before),
//         ),
//         bridge::bridge_cost_atom_bound_forward_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::CostAtomBoundForward, finish_top_shape(before),
//         ),
//         bridge::bridge_cost_signature_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::CostSignature, finish_top_shape(before),
//         ),
//         bridge::bridge_cost_region_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::CostRegion, finish_top_shape(before),
//         ),
//         bridge::bridge_cost_token_stack_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::CostTokenStack, finish_top_shape(before),
//         ),
// {
// }

// Disabled: replaced by the per-candidate finish_guard_* lemmas, one step body per query; a whole group in one query took up to 27,178,823 units (group 4).
// pub proof fn finish_selector_group_2_guard_sound(before: bridge::ScoreAdmissionBridge)
//     ensures
//         bridge::bridge_bundle_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::Bundle, finish_top_shape(before),
//         ),
//         bridge::bridge_if_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::If, finish_top_shape(before),
//         ),
//         bridge::bridge_send_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::Send, finish_top_shape(before),
//         ),
//         bridge::bridge_match_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::Match, finish_top_shape(before),
//         ),
//         bridge::bridge_absent_specialized_constructor_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::AbsentSpecializedConstructor, finish_top_shape(before),
//         ),
//         bridge::bridge_absent_connective_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::AbsentConnective, finish_top_shape(before),
//         ),
//         bridge::bridge_receive_bind_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ReceiveBind, finish_top_shape(before),
//         ),
//         bridge::bridge_match_case_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::MatchCase, finish_top_shape(before),
//         ),
// {
// }

// Disabled: replaced by the per-candidate finish_guard_* lemmas, one step body per query; a whole group in one query took up to 27,178,823 units (group 4).
// pub proof fn finish_selector_group_3_guard_sound(before: bridge::ScoreAdmissionBridge)
//     ensures
//         bridge::bridge_bool_oneof_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::BoolOneof, finish_top_shape(before),
//         ),
//         bridge::bridge_expr_atom_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ExprAtom, finish_top_shape(before),
//         ),
//         bridge::bridge_expr_numeric_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ExprNumeric, finish_top_shape(before),
//         ),
//         bridge::bridge_expr_big_rational_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ExprBigRational, finish_top_shape(before),
//         ),
//         bridge::bridge_expr_fixed_point_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ExprFixedPoint, finish_top_shape(before),
//         ),
//         bridge::bridge_sys_auth_token_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::SysAuthToken, finish_top_shape(before),
//         ),
//         bridge::bridge_new_empty_collections_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::NewEmptyCollections, finish_top_shape(before),
//         ),
//         bridge::bridge_new_one_injection_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::NewOneInjection, finish_top_shape(before),
//         ),
// {
// }

// Disabled: replaced by the per-candidate finish_guard_* lemmas, one step body per query; a whole group in one query took up to 27,178,823 units (group 4).
// pub proof fn finish_selector_group_4_guard_sound(before: bridge::ScoreAdmissionBridge)
//     ensures
//         bridge::bridge_new_one_absent_injection_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::NewOneAbsentInjection, finish_top_shape(before),
//         ),
//         bridge::bridge_new_one_uri_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::NewOneUri, finish_top_shape(before),
//         ),
//         bridge::bridge_new_collections_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::NewCollections, finish_top_shape(before),
//         ),
//         bridge::bridge_expr_payload_forward_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ExprPayloadForward, finish_top_shape(before),
//         ),
//         bridge::bridge_expr_structural_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ExprStructural, finish_top_shape(before),
//         ),
//         bridge::bridge_evar_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::EVar, finish_top_shape(before),
//         ),
//         bridge::bridge_ordered_sequence_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::OrderedSequence, finish_top_shape(before),
//         ),
//         bridge::bridge_emethod_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::EMethod, finish_top_shape(before),
//         ),
//         bridge::bridge_emap_finish_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::EMapFinish, finish_top_shape(before),
//         ),
// {
// }

// Disabled: replaced by the per-candidate finish_guard_* lemmas, one step body per query; a whole group in one query took up to 27,178,823 units (group 4).
// pub proof fn finish_selector_group_5_guard_sound(before: bridge::ScoreAdmissionBridge)
//     ensures
//         bridge::bridge_connective_not_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ConnectiveNot, finish_top_shape(before),
//         ),
//         bridge::bridge_connective_varref_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ConnectiveVarRef, finish_top_shape(before),
//         ),
//         bridge::bridge_connective_collection_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ConnectiveCollection, finish_top_shape(before),
//         ),
//         bridge::bridge_unforgeable_bytes_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::UnforgeableBytes, finish_top_shape(before),
//         ),
//         bridge::bridge_receive_present_condition_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ReceivePresentCondition, finish_top_shape(before),
//         ),
//         bridge::bridge_receive_absent_condition_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::ReceiveAbsentCondition, finish_top_shape(before),
//         ),
//         bridge::bridge_nonsemantic_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::Nonsemantic, finish_top_shape(before),
//         ),
// {
// }

// Disabled: replaced by the per-candidate finish_guard_* lemmas, one step body per query; a whole group in one query took up to 27,178,823 units (group 4).
// pub proof fn finish_selector_group_6_guard_sound(before: bridge::ScoreAdmissionBridge)
//     ensures
//         bridge::bridge_var_score_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::VarScore, finish_top_shape(before),
//         ),
//         bridge::bridge_epathmap_owner_score_step(before).is_some()
//             ==> message_finish_route_frame_guard(
//             MessageFinishRoute::EPathMapOwnerScore, finish_top_shape(before),
//         ),
//         bridge::bridge_ezipper_score_step(before).is_some() ==> message_finish_route_frame_guard(
//             MessageFinishRoute::EZipperScore, finish_top_shape(before),
//         ),
// {
// }

/// Group 1: `bridge_raw_par_step` fires only when the `RawPar` frame guard holds.
pub proof fn finish_guard_raw_par(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_raw_par(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::RawPar, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_raw_par);
}

/// Group 1: `bridge_cost_atom_absent_step` fires only when the `CostAtomAbsent` frame guard holds.
pub proof fn finish_guard_cost_atom_absent(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_cost_atom_absent(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::CostAtomAbsent, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_cost_atom_absent);
}

/// Group 1: `bridge_cost_atom_bytes_step` fires only when the `CostAtomBytes` frame guard holds.
pub proof fn finish_guard_cost_atom_bytes(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_cost_atom_bytes(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::CostAtomBytes, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_cost_atom_bytes);
}

/// Group 1: `bridge_cost_bound_step` fires only when the `CostBound` frame guard holds.
pub proof fn finish_guard_cost_bound(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_cost_bound(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::CostBound, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_cost_bound);
}

/// Group 1: `bridge_cost_atom_bound_forward_step` fires only when the `CostAtomBoundForward` frame
/// guard holds.
pub proof fn finish_guard_cost_atom_bound_forward(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_cost_atom_bound_forward(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::CostAtomBoundForward, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_cost_atom_bound_forward);
}

/// Group 1: `bridge_cost_signature_step` fires only when the `CostSignature` frame guard holds.
pub proof fn finish_guard_cost_signature(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_cost_signature(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::CostSignature, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_cost_signature);
}

/// Group 1: `bridge_cost_region_step` fires only when the `CostRegion` frame guard holds.
pub proof fn finish_guard_cost_region(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_cost_region(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::CostRegion, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_cost_region);
}

/// Group 1: `bridge_cost_token_stack_step` fires only when the `CostTokenStack` frame guard holds.
pub proof fn finish_guard_cost_token_stack(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_cost_token_stack(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::CostTokenStack, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_cost_token_stack);
}

/// Group 2: `bridge_bundle_step` fires only when the `Bundle` frame guard holds.
pub proof fn finish_guard_bundle(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_bundle(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::Bundle, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_bundle);
}

/// Group 2: `bridge_if_step` fires only when the `If` frame guard holds.
pub proof fn finish_guard_if(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_if(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::If, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_if);
}

/// Group 2: `bridge_send_step` fires only when the `Send` frame guard holds.
pub proof fn finish_guard_send(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_send(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::Send, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_send);
}

/// Group 2: `bridge_match_step` fires only when the `Match` frame guard holds.
pub proof fn finish_guard_match_component(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_match_component(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::Match, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_match_component);
}

/// Group 2: `bridge_absent_specialized_constructor_step` fires only when the
/// `AbsentSpecializedConstructor` frame guard holds.
pub proof fn finish_guard_absent_specialized(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_absent_specialized(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::AbsentSpecializedConstructor, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_absent_specialized);
}

/// Group 2: `bridge_absent_connective_step` fires only when the `AbsentConnective` frame guard
/// holds.
pub proof fn finish_guard_absent_connective(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_absent_connective(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::AbsentConnective, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_absent_connective);
}

/// Group 2: `bridge_receive_bind_step` fires only when the `ReceiveBind` frame guard holds.
pub proof fn finish_guard_receive_bind(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_receive_bind(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ReceiveBind, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_receive_bind);
}

/// Group 2: `bridge_match_case_step` fires only when the `MatchCase` frame guard holds.
pub proof fn finish_guard_match_case(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_match_case(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::MatchCase, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_match_case);
}

/// Group 3: `bridge_bool_oneof_step` fires only when the `BoolOneof` frame guard holds.
pub proof fn finish_guard_bool_oneof(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_bool_oneof(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::BoolOneof, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_bool_oneof);
}

/// Group 3: `bridge_expr_atom_step` fires only when the `ExprAtom` frame guard holds.
pub proof fn finish_guard_expr_atom(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_expr_atom(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::ExprAtom, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_expr_atom);
}

/// Group 3: `bridge_expr_numeric_step` fires only when the `ExprNumeric` frame guard holds.
pub proof fn finish_guard_expr_numeric(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_expr_numeric(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ExprNumeric, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_expr_numeric);
}

/// Group 3: `bridge_expr_big_rational_step` fires only when the `ExprBigRational` frame guard
/// holds.
pub proof fn finish_guard_expr_big_rational(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_expr_big_rational(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ExprBigRational, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_expr_big_rational);
}

/// Group 3: `bridge_expr_fixed_point_step` fires only when the `ExprFixedPoint` frame guard holds.
pub proof fn finish_guard_expr_fixed_point(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_expr_fixed_point(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ExprFixedPoint, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_expr_fixed_point);
}

/// Group 3: `bridge_sys_auth_token_step` fires only when the `SysAuthToken` frame guard holds.
pub proof fn finish_guard_sys_auth_token(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_sys_auth_token(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::SysAuthToken, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_sys_auth_token);
}

/// Group 3: `bridge_new_empty_collections_step` fires only when the `NewEmptyCollections` frame
/// guard holds.
pub proof fn finish_guard_new_empty_collections(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_new_empty_collections(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::NewEmptyCollections, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_new_empty_collections);
}

/// Group 3: `bridge_new_one_injection_step` fires only when the `NewOneInjection` frame guard
/// holds.
pub proof fn finish_guard_new_one_injection(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_new_one_injection(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::NewOneInjection, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_new_one_injection);
}

/// Group 4: `bridge_new_one_absent_injection_step` fires only when the `NewOneAbsentInjection`
/// frame guard holds.
pub proof fn finish_guard_new_one_absent_injection(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_new_one_absent_injection(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::NewOneAbsentInjection, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_new_one_absent_injection);
}

/// Group 4: `bridge_new_one_uri_step` fires only when the `NewOneUri` frame guard holds.
pub proof fn finish_guard_new_one_uri(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_new_one_uri(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::NewOneUri, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_new_one_uri);
}

/// Group 4: `bridge_new_collections_step` fires only when the `NewCollections` frame guard holds.
pub proof fn finish_guard_new_collections(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_new_collections(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::NewCollections, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_new_collections);
}

/// Group 4: `bridge_expr_payload_forward_step` fires only when the `ExprPayloadForward` frame guard
/// holds.
pub proof fn finish_guard_expr_payload_forward(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_expr_payload_forward(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ExprPayloadForward, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_expr_payload_forward);
}

/// Group 4: `bridge_expr_structural_step` fires only when the `ExprStructural` frame guard holds.
pub proof fn finish_guard_expr_structural(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_expr_structural(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ExprStructural, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_expr_structural);
}

/// Group 4: `bridge_evar_step` fires only when the `EVar` frame guard holds.
pub proof fn finish_guard_evar(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_evar(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::EVar, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_evar);
}

/// Group 4: `bridge_ordered_sequence_step` fires only when the `OrderedSequence` frame guard holds.
pub proof fn finish_guard_ordered_sequence(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_ordered_sequence(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::OrderedSequence, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_ordered_sequence);
}

/// Group 4: `bridge_emethod_step` fires only when the `EMethod` frame guard holds.
pub proof fn finish_guard_emethod(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_emethod(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::EMethod, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_emethod);
}

/// Group 4: `bridge_emap_finish_step` fires only when the `EMapFinish` frame guard holds.
pub proof fn finish_guard_emap_finish(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_emap_finish(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::EMapFinish, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_emap_finish);
}

/// Group 5: `bridge_connective_not_step` fires only when the `ConnectiveNot` frame guard holds.
pub proof fn finish_guard_connective_not(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_connective_not(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ConnectiveNot, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_connective_not);
}

/// Group 5: `bridge_connective_varref_step` fires only when the `ConnectiveVarRef` frame guard
/// holds.
pub proof fn finish_guard_connective_varref(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_connective_varref(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ConnectiveVarRef, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_connective_varref);
}

/// Group 5: `bridge_connective_collection_step` fires only when the `ConnectiveCollection` frame
/// guard holds.
pub proof fn finish_guard_connective_collection(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_connective_collection(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ConnectiveCollection, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_connective_collection);
}

/// Group 5: `bridge_unforgeable_bytes_step` fires only when the `UnforgeableBytes` frame guard
/// holds.
pub proof fn finish_guard_unforgeable_bytes(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_unforgeable_bytes(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::UnforgeableBytes, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_unforgeable_bytes);
}

/// Group 5: `bridge_receive_present_condition_step` fires only when the `ReceivePresentCondition`
/// frame guard holds.
pub proof fn finish_guard_receive_present_condition(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_receive_present_condition(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ReceivePresentCondition, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_receive_present_condition);
}

/// Group 5: `bridge_receive_absent_condition_step` fires only when the `ReceiveAbsentCondition`
/// frame guard holds.
pub proof fn finish_guard_receive_absent_condition(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_receive_absent_condition(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::ReceiveAbsentCondition, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_receive_absent_condition);
}

/// Group 5: `bridge_nonsemantic_step` fires only when the `Nonsemantic` frame guard holds.
pub proof fn finish_guard_nonsemantic(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_nonsemantic(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::Nonsemantic, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_nonsemantic);
}

/// Group 6: `bridge_var_score_step` fires only when the `VarScore` frame guard holds.
pub proof fn finish_guard_var(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_var(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::VarScore, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_var);
}

/// Group 6: `bridge_epathmap_owner_score_step` fires only when the `EPathMapOwnerScore` frame guard
/// holds.
pub proof fn finish_guard_epathmap_owner(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_epathmap_owner(before).is_some()
            ==> message_finish_route_frame_guard(
            MessageFinishRoute::EPathMapOwnerScore, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_epathmap_owner);
}

/// Group 6: `bridge_ezipper_score_step` fires only when the `EZipperScore` frame guard holds.
pub proof fn finish_guard_ezipper(before: bridge::ScoreAdmissionBridge)
    ensures
        groups::finish_candidate_ezipper(before).is_some() ==> message_finish_route_frame_guard(
            MessageFinishRoute::EZipperScore, finish_top_shape(before),
        ),
{
    reveal(groups::finish_candidate_ezipper);
}
pub proof fn finish_selector_group_1_empty(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires !finish_frame_has_selector_route(finish_top_shape(before)),
    ensures bridge::bridge_score_finish_selection_group_1(before, selection) == selection,
{
    groups::finish_group_1_body(before, selection);
    finish_guard_raw_par(before);
    finish_guard_cost_atom_absent(before);
    finish_guard_cost_atom_bytes(before);
    finish_guard_cost_bound(before);
    finish_guard_cost_atom_bound_forward(before);
    finish_guard_cost_signature(before);
    finish_guard_cost_region(before);
    finish_guard_cost_token_stack(before);
}

pub proof fn finish_selector_group_2_empty(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires !finish_frame_has_selector_route(finish_top_shape(before)),
    ensures bridge::bridge_score_finish_selection_group_2(before, selection) == selection,
{
    groups::finish_group_2_body(before, selection);
    finish_guard_bundle(before);
    finish_guard_if(before);
    finish_guard_send(before);
    finish_guard_match_component(before);
    finish_guard_absent_specialized(before);
    finish_guard_absent_connective(before);
    finish_guard_receive_bind(before);
    finish_guard_match_case(before);
}

pub proof fn finish_selector_group_3_empty(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires !finish_frame_has_selector_route(finish_top_shape(before)),
    ensures bridge::bridge_score_finish_selection_group_3(before, selection) == selection,
{
    groups::finish_group_3_body(before, selection);
    finish_guard_bool_oneof(before);
    finish_guard_expr_atom(before);
    finish_guard_expr_numeric(before);
    finish_guard_expr_big_rational(before);
    finish_guard_expr_fixed_point(before);
    finish_guard_sys_auth_token(before);
    finish_guard_new_empty_collections(before);
    finish_guard_new_one_injection(before);
}

pub proof fn finish_selector_group_4_empty(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires !finish_frame_has_selector_route(finish_top_shape(before)),
    ensures bridge::bridge_score_finish_selection_group_4(before, selection) == selection,
{
    groups::finish_group_4_body(before, selection);
    finish_guard_new_one_absent_injection(before);
    finish_guard_new_one_uri(before);
    finish_guard_new_collections(before);
    finish_guard_expr_payload_forward(before);
    finish_guard_expr_structural(before);
    finish_guard_evar(before);
    finish_guard_ordered_sequence(before);
    finish_guard_emethod(before);
    finish_guard_emap_finish(before);
}

pub proof fn finish_selector_group_5_empty(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires !finish_frame_has_selector_route(finish_top_shape(before)),
    ensures bridge::bridge_score_finish_selection_group_5(before, selection) == selection,
{
    groups::finish_group_5_body(before, selection);
    finish_guard_connective_not(before);
    finish_guard_connective_varref(before);
    finish_guard_connective_collection(before);
    finish_guard_unforgeable_bytes(before);
    finish_guard_receive_present_condition(before);
    finish_guard_receive_absent_condition(before);
    finish_guard_nonsemantic(before);
}

pub proof fn finish_selector_group_6_empty(
    before: bridge::ScoreAdmissionBridge, selection: bridge::BridgeScoreFinishSelection,
)
    requires !finish_frame_has_selector_route(finish_top_shape(before)),
    ensures bridge::bridge_score_finish_selection_group_6(before, selection) == selection,
{
    groups::finish_group_6_body(before, selection);
    finish_guard_var(before);
    finish_guard_epathmap_owner(before);
    finish_guard_ezipper(before);
}

pub proof fn finish_selector_none_without_guard(before: bridge::ScoreAdmissionBridge)
    requires !finish_frame_has_selector_route(finish_top_shape(before)),
    ensures bridge::bridge_score_finish_selected(before).is_none(),
{
    let empty = bridge::BridgeScoreFinishSelection::NoCandidate;
    finish_selector_group_1_empty(before, empty);
    finish_selector_group_2_empty(before, empty);
    finish_selector_group_3_empty(before, empty);
    finish_selector_group_4_empty(before, empty);
    finish_selector_group_5_empty(before, empty);
    finish_selector_group_6_empty(before, empty);
    reveal(bridge::bridge_score_finish_selected);
}

pub proof fn finish_noscore_guard_sound(before: runtime::RuntimeFrameImage)
    ensures
        runtime::runtime_noscore_child_kind(before).is_some() ==> {
            &&& before.score.admission.frames.len() > 1
            &&& message_finish_route_guard(
                finish_noscore_route_for(
                    finish_frame_shape(before.score.admission.frames.last()),
                ),
                finish_state_prestate(before.score.admission),
            )
        },
{
}

pub proof fn finish_noscore_root_guard_sound(before: runtime::RuntimeFrameImage)
    ensures
        runtime::runtime_noscore_root_return(before).is_some() ==> {
            &&& before.score.admission.frames.len() == 1
            &&& message_finish_route_guard(
                MessageFinishRoute::NoScoreRoot,
                finish_state_prestate(before.score.admission),
            )
        },
{
    runtime::runtime_noscore_root_return_is_checked(before);
    if runtime::runtime_noscore_root_return(before).is_some() {
        let state = before.score.admission;
        let root = state.frames[0];
        reveal(admission::admission_root_derived_summary);
        assert(admission::admission_frame_wf(root));
        assert(state.frames.last() == root);
        assert(finish_noscore_root_frame(finish_frame_shape(root)));
    }
}

pub proof fn finish_epathmap_guard_sound(
    before: runtime::RuntimeFrameImage, snapshot_bytes: Seq<u8>,
)
    ensures
        runtime::runtime_epm_source_score_finish_selected(before, snapshot_bytes).is_some() ==> {
            &&& before.score.admission.frames.len() > 0
            &&& message_finish_route_guard(
                MessageFinishRoute::EPathMapSnapshot,
                finish_state_prestate(before.score.admission),
            )
        },
{
}

pub proof fn finish_eof_route_is_mapped(
    before: runtime::RuntimeFrameImage, snapshot_bytes: Seq<u8>, program: u16,
    position: MessageFramePosition,
)
    requires
        before.score.admission.frames.len() > 0,
        generated_message_position(program, position),
        message_finish_prestate(program, position)
            == Some(finish_state_prestate(before.score.admission)),
    ensures
        runtime::runtime_noscore_child_kind(before).is_some() ==> message_finish_route_admitted(
            program, ProductionScoreOutcome::Unscored, position,
            finish_noscore_route_for(finish_frame_shape(before.score.admission.frames.last())),
        ),
        runtime::runtime_noscore_root_return(before).is_some() ==> message_finish_route_admitted(
            program, ProductionScoreOutcome::Unscored, position, MessageFinishRoute::NoScoreRoot,
        ),
        bridge::bridge_score_finish_selected(before.score).is_some()
            ==> exists|route: MessageFinishRoute|
            message_finish_route_is_selector(route) && #[trigger] message_finish_route_admitted(
                program, ProductionScoreOutcome::Scored, position, route,
            ),
        runtime::runtime_epm_source_score_finish_selected(before, snapshot_bytes).is_some()
            ==> message_finish_route_admitted(
            program, ProductionScoreOutcome::Scored, position,
            MessageFinishRoute::EPathMapSnapshot,
        ),
{
    let prestate = finish_state_prestate(before.score.admission);
    finish_noscore_guard_sound(before);
    if runtime::runtime_noscore_child_kind(before).is_some() {
        let route = finish_noscore_route_for(
            finish_frame_shape(before.score.admission.frames.last()),
        );
        finish_mapping_routes_complete(program, position, route);
    }
    finish_noscore_root_guard_sound(before);
    if runtime::runtime_noscore_root_return(before).is_some() {
        finish_mapping_routes_complete(program, position, MessageFinishRoute::NoScoreRoot);
    }
    if bridge::bridge_score_finish_selected(before.score).is_some() {
        if !finish_frame_has_selector_route(finish_top_shape(before.score)) {
            finish_selector_none_without_guard(before.score);
        }
        let route = choose|route: MessageFinishRoute|
            message_finish_route_is_selector(route)
                && #[trigger] message_finish_route_frame_guard(
                route, finish_top_shape(before.score),
            );
        assert(message_finish_route_guard(route, prestate));
        finish_mapping_routes_complete(program, position, route);
        assert(message_finish_route_admitted(
            program, ProductionScoreOutcome::Scored, position, route,
        ));
    }
    finish_epathmap_guard_sound(before, snapshot_bytes);
    if runtime::runtime_epm_source_score_finish_selected(before, snapshot_bytes).is_some() {
        finish_mapping_routes_complete(
            program, position, MessageFinishRoute::EPathMapSnapshot,
        );
    }
}

// Disabled: false; a program-2 root returns unscored and a program-6 top selects VarScore (finish_eof_route_is_mapped).
// pub proof fn finish_eof_rejects_no_route_programs(
//     before: runtime::RuntimeFrameImage, snapshot_bytes: Seq<u8>,
// )
//     requires
//         before.score.admission.frames.len() > 0,
//         before.score.admission.frames.last().family == admission::AdmissionFamily::Other,
//         finish_no_route_program(before.score.admission.frames.last().program_type_id),
//     ensures
//         runtime::runtime_noscore_child_kind(before).is_none(),
//         bridge::bridge_score_finish_selected(before.score).is_none(),
//         runtime::runtime_epm_source_score_finish_selected(before, snapshot_bytes).is_none(),
//         runtime::runtime_message_driver_step(before, runtime::RuntimeMessageEvent::Eof)
//             .is_none(),
//         runtime::runtime_message_driver_step(
//             before, runtime::RuntimeMessageEvent::EofEPathMap { snapshot_bytes },
//         ).is_none(),
// {
//     let shape = finish_top_shape(before.score);
//     assert forall|route: MessageFinishRoute|
//         !(message_finish_route_is_selector(route)
//             && #[trigger] message_finish_route_frame_guard(route, shape)) by {
//     }
//     finish_selector_none_without_guard(before.score);
// }

// Disabled: false; the incomplete EMap child under Expr tag 23 takes its no-score arm (finish_nonvacuity_emap_incomplete_child).
// pub proof fn finish_eof_rejects_incomplete_emap_child(
//     before: runtime::RuntimeFrameImage, snapshot_bytes: Seq<u8>,
// )
//     requires
//         before.score.admission.frames.len() > 1,
//         admission::admission_frame_wf(before.score.admission.frames.last()),
//         before.score.admission.frames.last().family == admission::AdmissionFamily::Other,
//         before.score.admission.frames.last().program_type_id == 30,
//         before.score.admission.frames.last().frame_kind == 1,
//         match bridge::bridge_emap_finish_source(before.score) {
//             Some(source) => !source.complete,
//             None => false,
//         },
//     ensures
//         runtime::runtime_noscore_child_kind(before).is_none(),
//         bridge::bridge_score_finish_selected(before.score).is_none(),
//         runtime::runtime_epm_source_score_finish_selected(before, snapshot_bytes).is_none(),
//         runtime::runtime_message_driver_step(before, runtime::RuntimeMessageEvent::Eof)
//             .is_none(),
//         runtime::runtime_message_driver_step(
//             before, runtime::RuntimeMessageEvent::EofEPathMap { snapshot_bytes },
//         ).is_none(),
// {
//     assert(bridge::bridge_structural_no_score_child_frame(before.score));
//     bridge::bridge_structural_no_score_child_finish_selected_none(before.score);
// }

// Disabled: false; the root return no longer requires a component tag (runtime_par_root_return_witness).
// /// Observation on the pop side, outside the finish-branch statement. A root
// /// without a component tag (RawPar, EMap, ...) can finish through the selector
// /// (`RootScore`), but the Eof event never returns it. `RootComplete` requires
// /// that return, and `modeled_message_finish_action` admits only component
// /// roots. The EPathMap root uses `EofEPathMap` and is not affected.
// pub proof fn finish_observation_root_return_needs_component(before: runtime::RuntimeFrameImage)
//     requires
//         runtime::runtime_frame_image_wf(before),
//         before.score.admission.frames.len() == 1,
//         finish::component_tag_for_family(before.score.admission.frames[0].family).is_none(),
//     ensures
//         match runtime::runtime_message_driver_step(before, runtime::RuntimeMessageEvent::Eof) {
//             Some(runtime::RuntimeMessageOutcome::RootReturned { .. }) => false,
//             _ => true,
//         },
// {
//     runtime::runtime_selected_score_finish_is_checked(before);
// }

pub proof fn finish_nonvacuity_big_rational_child()
    ensures
        match generated::generated_descriptor(24, 36) {
            Some(descriptor) => descriptor.frame_kind == 1
                && descriptor.child_type_id == Some(25u16) && descriptor.slot == 0,
            None => false,
        },
        generated_message_position(
            25, MessageFramePosition::Child { parent_program: 24, field_tag: 36 },
        ),
        production_semantic_kind(25) == Some(ProductionSemanticKind::BigRational),
        message_finish_mapping(
            25, ProductionScoreOutcome::Unscored,
            MessageFramePosition::Child { parent_program: 24, field_tag: 36 },
        ) == Some(MessageFinishRoute::NoScoreBigRational),
        message_finish_prestate(
            25, MessageFramePosition::Child { parent_program: 24, field_tag: 36 },
        ) == Some(finish_state_prestate(
            runtime::runtime_noscore_big_rational_before().score.admission,
        )),
        runtime::runtime_noscore_child_kind(runtime::runtime_noscore_big_rational_before())
            .is_some(),
        finish_noscore_route_for(finish_frame_shape(
            runtime::runtime_noscore_big_rational_before().score.admission.frames.last(),
        )) == MessageFinishRoute::NoScoreBigRational,
        match runtime::runtime_message_driver_step(
            runtime::runtime_noscore_big_rational_before(), runtime::RuntimeMessageEvent::Eof,
        ) {
            Some(runtime::RuntimeMessageOutcome::ParentAccepted { .. }) => true,
            _ => false,
        },
{
    runtime::runtime_noscore_big_rational_modeled_witness();
    let state = runtime::runtime_noscore_big_rational_before().score.admission;
    assert(state.frames.len() == 2);
    assert(state.frames.last() == state.frames[1]);
}

pub proof fn finish_nonvacuity_empty_send_root()
    ensures
        generated_message_position(15, MessageFramePosition::Root),
        production_semantic_kind(15)
            == Some(ProductionSemanticKind::Component(ProductionParComponentTag::Send)),
        message_finish_prestate(15, MessageFramePosition::Root)
            == Some(finish_state_prestate(runtime::runtime_empty_send_before().score.admission)),
        bridge::bridge_nonsemantic_step(runtime::runtime_empty_send_before().score).is_some(),
        message_finish_route_admitted(
            15, ProductionScoreOutcome::Scored, MessageFramePosition::Root,
            MessageFinishRoute::Nonsemantic,
        ),
        bridge::bridge_score_finish_selected(runtime::runtime_empty_send_before().score)
            == bridge::bridge_nonsemantic_finish_candidate(
            runtime::runtime_empty_send_before().score,
        ),
        match runtime::runtime_message_driver_step(
            runtime::runtime_empty_send_before(), runtime::RuntimeMessageEvent::Eof,
        ) {
            Some(runtime::RuntimeMessageOutcome::RootReturned { .. }) => true,
            _ => false,
        },
{
    finish::empty_send_finish_has_root_return();
    runtime::runtime_selected_empty_send_return_is_reachable();
    let before = runtime::runtime_empty_send_before();
    let root = finish::empty_send_finish_root();
    assert(admission::admission_root_initial(root));
    assert(admission::admission_frame_wf(root));
    assert(before.score.admission.frames.len() == 1);
    assert(before.score.admission.frames.last() == root);
    assert(bridge::bridge_primary_component_frame(before.score));
    bridge::bridge_primary_component_finish_selected_exact(before.score);
    assert(!admission::admission_family_complete(root));
    assert(bridge::bridge_send_step(before.score).is_none());
    assert(bridge::bridge_primary_component_finish(before.score).is_none());
}

pub proof fn finish_nonvacuity_unit_root()
    ensures
        generated_message_position(7, MessageFramePosition::Root),
        production_semantic_kind(7) == Some(ProductionSemanticKind::Unit),
        message_finish_mapping(7, ProductionScoreOutcome::Unscored, MessageFramePosition::Root)
            == Some(MessageFinishRoute::NoScoreRoot),
        message_finish_prestate(7, MessageFramePosition::Root) == Some(finish_state_prestate(
            runtime::runtime_noscore_unit_root_before().score.admission,
        )),
        runtime::runtime_noscore_root_return(runtime::runtime_noscore_unit_root_before())
            .is_some(),
        match runtime::runtime_message_driver_step(
            runtime::runtime_noscore_unit_root_before(), runtime::RuntimeMessageEvent::Eof,
        ) {
            Some(runtime::RuntimeMessageOutcome::RootReturnedNoScore { .. }) => true,
            _ => false,
        },
{
    runtime::runtime_noscore_unit_root_witness();
    let state = runtime::runtime_noscore_unit_root_before().score.admission;
    assert(state.frames.len() == 1);
    assert(state.frames.last() == admission::admission_unit_noscore_root());
}

pub proof fn finish_nonvacuity_var_child()
    ensures
        match generated::generated_descriptor(35, 1) {
            Some(descriptor) => descriptor.frame_kind == 1
                && descriptor.child_type_id == Some(6u16) && descriptor.slot == 0,
            None => false,
        },
        production_semantic_kind(6) == Some(ProductionSemanticKind::Var),
        message_finish_mapping(
            6, ProductionScoreOutcome::Scored,
            MessageFramePosition::Child { parent_program: 35, field_tag: 1 },
        ) == Some(MessageFinishRoute::VarScore),
        message_finish_prestate(
            6, MessageFramePosition::Child { parent_program: 35, field_tag: 1 },
        ) == Some(finish_state_prestate(bridge::bridge_var_child_witness_before().admission)),
        bridge::bridge_var_score_step(bridge::bridge_var_child_witness_before()).is_some(),
        bridge::bridge_score_finish_selected(bridge::bridge_var_child_witness_before())
            .is_some(),
{
    bridge::bridge_score_finish_selection_group_6_var_child_before_wf();
    bridge::bridge_score_finish_selection_group_6_var_child_witness();
    let state = bridge::bridge_var_child_witness_before().admission;
    assert(state.frames.len() == 2);
    assert(state.frames[0] == bridge::bridge_var_child_witness_parent());
    assert(state.frames.last() == bridge::bridge_var_child_witness_child());
}

pub proof fn finish_nonvacuity_emap_incomplete_child()
    ensures
        generated_message_position(
            30, MessageFramePosition::Child { parent_program: 24, field_tag: 23 },
        ),
        production_semantic_kind(30) == Some(ProductionSemanticKind::EMap),
        message_finish_mapping(
            30, ProductionScoreOutcome::Unscored,
            MessageFramePosition::Child { parent_program: 24, field_tag: 23 },
        ) == Some(MessageFinishRoute::NoScoreEMapChild),
        message_finish_prestate(
            30, MessageFramePosition::Child { parent_program: 24, field_tag: 23 },
        ) == Some(finish_state_prestate(
            runtime::runtime_noscore_emap_incomplete_before().score.admission,
        )),
        runtime::runtime_noscore_child_kind(runtime::runtime_noscore_emap_incomplete_before())
            .is_some(),
        finish_noscore_route_for(finish_frame_shape(
            runtime::runtime_noscore_emap_incomplete_before().score.admission.frames.last(),
        )) == MessageFinishRoute::NoScoreEMapChild,
        match runtime::runtime_message_driver_step(
            runtime::runtime_noscore_emap_incomplete_before(), runtime::RuntimeMessageEvent::Eof,
        ) {
            Some(runtime::RuntimeMessageOutcome::ParentAccepted { .. }) => true,
            _ => false,
        },
{
    runtime::runtime_noscore_emap_incomplete_modeled_witness();
    let state = runtime::runtime_noscore_emap_incomplete_before().score.admission;
    assert(state.frames.len() == 2);
    assert(state.frames.last() == state.frames[1]);
}

pub proof fn finish_nonvacuity_ezipper_absent_child()
    ensures
        generated_message_position(
            32, MessageFramePosition::Child { parent_program: 24, field_tag: 33 },
        ),
        production_semantic_kind(32) == Some(ProductionSemanticKind::EZipper),
        message_finish_mapping(
            32, ProductionScoreOutcome::Unscored,
            MessageFramePosition::Child { parent_program: 24, field_tag: 33 },
        ) == Some(MessageFinishRoute::NoScoreEZipperChild),
        message_finish_prestate(
            32, MessageFramePosition::Child { parent_program: 24, field_tag: 33 },
        ) == Some(finish_state_prestate(
            runtime::runtime_noscore_ezipper_absent_before().score.admission,
        )),
        runtime::runtime_noscore_child_kind(runtime::runtime_noscore_ezipper_absent_before())
            .is_some(),
        finish_noscore_route_for(finish_frame_shape(
            runtime::runtime_noscore_ezipper_absent_before().score.admission.frames.last(),
        )) == MessageFinishRoute::NoScoreEZipperChild,
        match runtime::runtime_message_driver_step(
            runtime::runtime_noscore_ezipper_absent_before(), runtime::RuntimeMessageEvent::Eof,
        ) {
            Some(runtime::RuntimeMessageOutcome::ParentAccepted { .. }) => true,
            _ => false,
        },
{
    runtime::runtime_noscore_ezipper_absent_modeled_witness();
    let state = runtime::runtime_noscore_ezipper_absent_before().score.admission;
    assert(state.frames.len() == 2);
    assert(state.frames.last() == state.frames[1]);
}

pub proof fn finish_nonvacuity_ezipper_scored_root()
    ensures
        generated_message_position(32, MessageFramePosition::Root),
        message_finish_mapping(32, ProductionScoreOutcome::Scored, MessageFramePosition::Root)
            == Some(MessageFinishRoute::EZipperScore),
        match bridge::bridge_ezipper_witness_after_child() {
            Some(before) => {
                &&& message_finish_prestate(32, MessageFramePosition::Root)
                    == Some(finish_state_prestate(before.admission))
                &&& bridge::bridge_ezipper_score_step(before).is_some()
                &&& bridge::bridge_score_finish_selected(before)
                    == bridge::bridge_ezipper_score_step(before)
            },
            None => false,
        },
{
    bridge::bridge_score_finish_selection_group_6_ezipper_witness();
    let before = bridge::bridge_ezipper_witness_after_child().unwrap();
    finish_guard_ezipper(before);
    reveal(groups::finish_candidate_ezipper);
    assert(before.admission.frames.len() == 1);
    assert(before.admission.frames.last() == before.admission.frames[0]);
}

} // verus!
