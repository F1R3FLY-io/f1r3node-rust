//! Strict, schema-driven admission of canonical Rholang protobuf bytes.
//!
//! Prost decoding is intentionally permissive: it accepts unknown fields,
//! duplicate singular fields, explicit defaults, and several scalar aliases
//! that encode back to different bytes. RPB3 keys require the stronger fixed
//! point: the supplied body must be exactly one image of the generated encoder.
//!
//! This module owns the wire half of that proof. It drives generated
//! [`RhoMatchProgram`] tables with one explicit heap frame stack, validates
//! scalar domains/default omission, descends through messages and maps, and
//! validates every nested EPM1 `Par` value. It never constructs a rhoapi AST.
//! The score half of RPB3 admission is deliberately layered on the same frame
//! summaries rather than implemented as a second protobuf traversal.

use std::fmt;

use pathmap::PathMap;

use super::canonical_score_bytes::{
    CanonicalScoreArena, CanonicalScoreArenaError, CanonicalScoreHandle,
};
use super::canonical_semantics::{
    self as semantics, CanonicalScoreAlgebra, ParComponentTag, Score,
};
use super::match_schema::{
    resolve_protobuf_field, wire_program_by_type_id, ResolvedProtobufField,
    ResolvedProtobufPayload, RhoMatchFieldKind, RhoMatchPayloadKind, RhoMatchProgram,
};
use super::match_schema_tables::*;
use super::protobuf_cursor::{
    CanonicalProtobufError, CanonicalRegionField, CanonicalRegionFieldCursor,
    CanonicalRegionFieldValue, CanonicalWireType, ProtobufRegion,
};
use super::protobuf_schema::{ProtobufKind, ProtobufWireType};
use crate::rust::epathmap_trie_codec::{
    OwnedCanonicalEpm1TopologyCursor, OwnedCanonicalEpm1TopologyEvent, TrieCodecError,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CanonicalProtobufAdmissionStats {
    pub(crate) fields: usize,
    pub(crate) messages: usize,
    pub(crate) map_entries: usize,
    pub(crate) epathmaps: usize,
    pub(crate) max_frames: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalProtobufAdmission {
    stats: CanonicalProtobufAdmissionStats,
    semantic_default_par: Option<bool>,
    canonical_path_stable_par: Option<bool>,
    component: Option<CanonicalComponentAdmission>,
}

impl CanonicalProtobufAdmission {
    pub(crate) const fn stats(self) -> CanonicalProtobufAdmissionStats { self.stats }

    /// `Some(true)` means the admitted root is a raw protobuf `Par` equal to
    /// `Par::default()`. Root `locally_free` is intentionally ignored, exactly
    /// as `Par::PartialEq` ignores it and Prost's map-value default test does.
    pub(crate) const fn semantic_default_par(self) -> Option<bool> { self.semantic_default_par }

    /// `Some(stable)` is the exact canonical-path stability verdict for an
    /// admitted raw `Par`.  Nested path admission uses this to resolve an
    /// escape payload without constructing a rhoapi value.
    pub(crate) const fn canonical_path_stable_par(self) -> Option<bool> {
        self.canonical_path_stable_par
    }

    pub(crate) const fn component(self) -> Option<CanonicalComponentAdmission> { self.component }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalComponentAdmission {
    pub(crate) family: ParComponentTag,
    pub(crate) canonical_path_stable: bool,
    pub(crate) complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalProtobufAdmissionError {
    Framing {
        type_name: &'static str,
        region_offset: usize,
        source: CanonicalProtobufError,
    },
    Schema {
        type_name: &'static str,
        field: u32,
        region_offset: usize,
        detail: &'static str,
    },
    EPathMap {
        type_name: &'static str,
        field: u32,
        region_offset: usize,
        source: TrieCodecError,
    },
    CanonicalPath {
        type_name: &'static str,
        field: u32,
        region_offset: usize,
        source_offset: usize,
        detail: String,
    },
    ScoreMismatch {
        type_name: &'static str,
        region_offset: usize,
        first_difference: usize,
    },
    SizeOverflow,
}

impl From<CanonicalScoreArenaError> for CanonicalProtobufAdmissionError {
    fn from(_: CanonicalScoreArenaError) -> Self { Self::SizeOverflow }
}

impl fmt::Display for CanonicalProtobufAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing {
                type_name,
                region_offset,
                source,
            } => write!(
                formatter,
                "{type_name} protobuf region at byte {region_offset}: {source}"
            ),
            Self::Schema {
                type_name,
                field,
                region_offset,
                detail,
            } => write!(
                formatter,
                "{type_name} protobuf field {field} in region at byte {region_offset}: {detail}"
            ),
            Self::EPathMap {
                type_name,
                field,
                region_offset,
                source,
            } => write!(
                formatter,
                "{type_name} protobuf field {field} EPM1 region at byte {region_offset}: {source}"
            ),
            Self::CanonicalPath {
                type_name,
                field,
                region_offset,
                source_offset,
                detail,
            } => write!(
                formatter,
                "{type_name} protobuf field {field} canonical path near snapshot byte {}: {detail}",
                region_offset.saturating_add(*source_offset),
            ),
            Self::ScoreMismatch {
                type_name,
                region_offset,
                first_difference,
            } => write!(
                formatter,
                "{type_name} canonical score differs at byte {}",
                region_offset.saturating_add(*first_difference),
            ),
            Self::SizeOverflow => formatter.write_str("canonical protobuf admission size overflow"),
        }
    }
}

impl std::error::Error for CanonicalProtobufAdmissionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing { source, .. } => Some(source),
            Self::EPathMap { source, .. } => Some(source),
            Self::Schema { .. }
            | Self::CanonicalPath { .. }
            | Self::ScoreMismatch { .. }
            | Self::SizeOverflow => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SemanticKind {
    RawPar,
    Component(ParComponentTag),
    Var,
    Unit,
    ReceiveBind,
    MatchCase,
    Expr,
    BigRational,
    FixedPoint,
    OrderedCollection {
        constructor: i32,
        has_remainder: bool,
        canonical_set: bool,
    },
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

fn semantic_kind(program: &'static RhoMatchProgram) -> Option<SemanticKind> {
    macro_rules! program_is {
        ($program:ident) => {
            std::ptr::eq(program, &$program)
        };
    }

    let kind = if program_is!(PAR_PROTOBUF_REGION_PROGRAM) {
        SemanticKind::RawPar
    } else if program_is!(SEND_RHO_MATCH_PROGRAM) {
        SemanticKind::Component(ParComponentTag::Send)
    } else if program_is!(RECEIVE_RHO_MATCH_PROGRAM) {
        SemanticKind::Component(ParComponentTag::Receive)
    } else if program_is!(NEW_RHO_MATCH_PROGRAM) {
        SemanticKind::Component(ParComponentTag::New)
    } else if program_is!(EXPR_RHO_MATCH_PROGRAM) {
        SemanticKind::Expr
    } else if program_is!(MATCH_RHO_MATCH_PROGRAM) {
        SemanticKind::Component(ParComponentTag::Match)
    } else if program_is!(GUNFORGEABLE_RHO_MATCH_PROGRAM) {
        SemanticKind::GUnforgeable
    } else if program_is!(CONNECTIVE_RHO_MATCH_PROGRAM) {
        SemanticKind::Connective
    } else if program_is!(BUNDLE_RHO_MATCH_PROGRAM) {
        SemanticKind::Component(ParComponentTag::Bundle)
    } else if program_is!(IF_RHO_MATCH_PROGRAM) {
        SemanticKind::Component(ParComponentTag::If)
    } else if program_is!(VAR_RHO_MATCH_PROGRAM) {
        SemanticKind::Var
    } else if program_is!(VAR_WILDCARDMSG_RHO_MATCH_PROGRAM)
        || program_is!(GSYSAUTHTOKEN_RHO_MATCH_PROGRAM)
    {
        SemanticKind::Unit
    } else if program_is!(RECEIVEBIND_RHO_MATCH_PROGRAM) {
        SemanticKind::ReceiveBind
    } else if program_is!(MATCHCASE_RHO_MATCH_PROGRAM) {
        SemanticKind::MatchCase
    } else if program_is!(GBIGRATIONAL_RHO_MATCH_PROGRAM) {
        SemanticKind::BigRational
    } else if program_is!(GFIXEDPOINT_RHO_MATCH_PROGRAM) {
        SemanticKind::FixedPoint
    } else if program_is!(ELIST_RHO_MATCH_PROGRAM) {
        SemanticKind::OrderedCollection {
            constructor: Score::ELIST,
            has_remainder: true,
            canonical_set: false,
        }
    } else if program_is!(ETUPLE_RHO_MATCH_PROGRAM) {
        SemanticKind::OrderedCollection {
            constructor: Score::ETUPLE,
            has_remainder: false,
            canonical_set: false,
        }
    } else if program_is!(ESET_RHO_MATCH_PROGRAM) {
        SemanticKind::OrderedCollection {
            constructor: Score::ESET,
            has_remainder: true,
            canonical_set: true,
        }
    } else if program_is!(EMAP_RHO_MATCH_PROGRAM) {
        SemanticKind::EMap
    } else if program_is!(EPATHMAP_RHO_MATCH_PROGRAM) {
        SemanticKind::EPathMap
    } else if program_is!(EZIPPER_RHO_MATCH_PROGRAM) {
        SemanticKind::EZipper
    } else if program_is!(EMETHOD_RHO_MATCH_PROGRAM) {
        SemanticKind::EMethod
    } else if program_is!(KEYVALUEPAIR_RHO_MATCH_PROGRAM) {
        SemanticKind::KeyValuePair
    } else if program_is!(EVAR_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprVar
    } else if program_is!(ENEG_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprUnary(Score::ENEG)
    } else if program_is!(ENOT_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprUnary(Score::ENOT)
    } else if program_is!(EMULT_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EMULT)
    } else if program_is!(EDIV_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EDIV)
    } else if program_is!(EMOD_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EMOD)
    } else if program_is!(EPLUS_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EPLUS)
    } else if program_is!(EMINUS_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EMINUS)
    } else if program_is!(ELT_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::ELT)
    } else if program_is!(ELTE_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::ELTE)
    } else if program_is!(EGT_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EGT)
    } else if program_is!(EGTE_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EGTE)
    } else if program_is!(EEQ_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EEQ)
    } else if program_is!(ENEQ_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::ENEQ)
    } else if program_is!(EAND_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EAND)
    } else if program_is!(EOR_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EOR)
    } else if program_is!(EMATCHES_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EMATCHES)
    } else if program_is!(EPERCENTPERCENT_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EPERCENT)
    } else if program_is!(EPLUSPLUS_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EPLUSPLUS)
    } else if program_is!(EMINUSMINUS_RHO_MATCH_PROGRAM) {
        SemanticKind::ExprBinary(Score::EMINUSMINUS)
    } else if program_is!(VARREF_RHO_MATCH_PROGRAM) {
        SemanticKind::VarRef
    } else if program_is!(CONNECTIVEBODY_RHO_MATCH_PROGRAM) {
        SemanticKind::ConnectiveBody
    } else if program_is!(GPRIVATE_RHO_MATCH_PROGRAM)
        || program_is!(GDEPLOYID_RHO_MATCH_PROGRAM)
        || program_is!(GDEPLOYERID_RHO_MATCH_PROGRAM)
    {
        SemanticKind::BytesLeaf
    } else {
        return None;
    };
    Some(kind)
}

#[derive(Clone)]
enum SemanticScalar {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    String(ProtobufRegion),
    Bytes(ProtobufRegion),
}

struct SemanticOccurrence {
    variant: Option<&'static str>,
    body: Option<ProtobufRegion>,
    value: SemanticValue,
    field_offset: usize,
}

enum SemanticValue {
    Scalar(SemanticScalar),
    Message(Box<MessageSummary>),
    Map(Box<MapSummary>),
    EPathMap(Box<EPathMapSummary>),
}

#[derive(Clone)]
struct ComponentSummary {
    family: ParComponentTag,
    score: CanonicalScoreHandle,
    complete: bool,
    canonical_path_stable: bool,
}

#[derive(Clone)]
struct ParSummary {
    score: CanonicalScoreHandle,
    semantic_default: bool,
    canonical_path_stable: bool,
    identity_prefix: ProtobufRegion,
    identity_suffix: ProtobufRegion,
}

#[derive(Clone)]
struct ExprPayloadSummary {
    score: Option<CanonicalScoreHandle>,
    canonical_path_stable: bool,
    canonical_fixed_point: bool,
    pathmap_snapshot: Option<ProtobufRegion>,
}

#[derive(Clone)]
enum SemanticSummary {
    Par(ParSummary),
    Component(ComponentSummary),
    Var(CanonicalScoreHandle),
    ReceiveBind {
        score: Option<CanonicalScoreHandle>,
        source_present: bool,
    },
    MatchCase {
        score: Option<CanonicalScoreHandle>,
        required_present: bool,
        canonical_fixed_point: bool,
    },
    ExprPayload(ExprPayloadSummary),
    KeyValuePair {
        key: Option<ParSummary>,
        value: Option<ParSummary>,
    },
    BigRational {
        numerator: ProtobufRegion,
        denominator: ProtobufRegion,
    },
    FixedPoint {
        unscaled: ProtobufRegion,
        scale: u32,
    },
    ConnectiveBody(Vec<ParSummary>),
    VarRef {
        index: i32,
        depth: i32,
    },
    Bytes(ProtobufRegion),
    Unit,
}

#[derive(Clone)]
struct EPathMapSummary {
    snapshot: ProtobufRegion,
    entries_canonical_path_stable: bool,
}

struct MessageFrame {
    program: &'static RhoMatchProgram,
    body: ProtobufRegion,
    region_offset: usize,
    cursor: CanonicalRegionFieldCursor,
    last_slot: Option<usize>,
    seen: Vec<bool>,
    previous_map_keys: Vec<Option<ProtobufRegion>>,
    previous_par_components: Vec<Option<(CanonicalScoreHandle, ProtobufRegion)>>,
    semantic_slots: Vec<Vec<SemanticOccurrence>>,
    waiting: Option<MessageWait>,
    par_components: u64,
    par_locally_free_range: Option<super::protobuf_cursor::CanonicalByteRange>,
}

enum MessageWait {
    Child {
        slot: usize,
        variant: Option<&'static str>,
        body: ProtobufRegion,
        field_offset: usize,
    },
    Map {
        slot: usize,
        field_offset: usize,
    },
    EPathMap {
        slot: usize,
        field_offset: usize,
    },
}

impl MessageFrame {
    fn new(
        region: ProtobufRegion,
        program: &'static RhoMatchProgram,
    ) -> Result<Self, CanonicalProtobufAdmissionError> {
        let region_offset = region.absolute_range().start;
        Self::new_at(region, program, region_offset)
    }

    fn new_at(
        region: ProtobufRegion,
        program: &'static RhoMatchProgram,
        region_offset: usize,
    ) -> Result<Self, CanonicalProtobufAdmissionError> {
        if program.fields.len() != program.protobuf_fields.len() {
            return Err(CanonicalProtobufAdmissionError::Schema {
                type_name: program.type_name,
                field: 0,
                region_offset,
                detail: "wire program field tables have different lengths",
            });
        }
        if program
            .fields
            .iter()
            .any(|field| matches!(field.kind, RhoMatchFieldKind::ParBag))
        {
            return Err(CanonicalProtobufAdmissionError::Schema {
                type_name: program.type_name,
                field: 0,
                region_offset,
                detail: "native ParBag program cannot validate protobuf wire bytes",
            });
        }
        let slots = program.fields.len();
        Ok(Self {
            program,
            body: region.clone(),
            region_offset,
            cursor: CanonicalRegionFieldCursor::in_prost_slot_order(region),
            last_slot: None,
            seen: vec![false; slots],
            previous_map_keys: vec![None; slots],
            previous_par_components: vec![None; slots],
            semantic_slots: (0..slots).map(|_| Vec::new()).collect(),
            waiting: None,
            par_components: 0,
            par_locally_free_range: None,
        })
    }

    fn is_raw_par(&self) -> bool { std::ptr::eq(self.program, &PAR_PROTOBUF_REGION_PROGRAM) }

    fn schema_error(&self, field: u32, detail: &'static str) -> CanonicalProtobufAdmissionError {
        self.schema_error_at(field, self.region_offset, detail)
    }

    fn schema_error_at(
        &self,
        field: u32,
        region_offset: usize,
        detail: &'static str,
    ) -> CanonicalProtobufAdmissionError {
        CanonicalProtobufAdmissionError::Schema {
            type_name: self.program.type_name,
            field,
            region_offset,
            detail,
        }
    }

    fn field_error(
        &self,
        field: &CanonicalRegionField,
        detail: &'static str,
    ) -> CanonicalProtobufAdmissionError {
        let absolute = self
            .region_offset
            .checked_add(field.encoded_range.start)
            .expect("a field relative offset remains inside its protobuf backing");
        self.schema_error_at(field.number, absolute, detail)
    }

    fn accept(
        &mut self,
        summary: FrameSummary,
        scores: &CanonicalScoreArena,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        match (self.waiting.take(), summary) {
            (
                Some(MessageWait::Child {
                    slot,
                    variant,
                    body,
                    field_offset,
                }),
                FrameSummary::Message(summary),
            ) => {
                if self.is_raw_par() {
                    let SemanticSummary::Component(component) = &summary.semantic else {
                        return Err(self.schema_error_at(
                            self.program.fields[slot].tag,
                            body.absolute_range().start,
                            "raw Par component child returned the wrong semantic summary",
                        ));
                    };
                    if let Some((previous_score, previous_body)) =
                        &self.previous_par_components[slot]
                    {
                        let order = scores
                            .compare(*previous_score, component.score)
                            .then_with(|| previous_body.as_bytes().cmp(body.as_bytes()));
                        if order.is_gt() {
                            return Err(self.schema_error_at(
                                self.program.fields[slot].tag,
                                body.absolute_range().start,
                                "raw Par components are outside canonical score/body order",
                            ));
                        }
                    }
                    self.previous_par_components[slot] = Some((component.score, body.clone()));
                }
                self.semantic_slots[slot].push(SemanticOccurrence {
                    variant,
                    body: Some(body),
                    value: SemanticValue::Message(Box::new(summary)),
                    field_offset,
                });
                Ok(())
            }
            (Some(MessageWait::Map { slot, field_offset }), FrameSummary::Map(entry)) => {
                if let Some(previous) = &self.previous_map_keys[slot] {
                    if previous.as_bytes() >= entry.key.as_bytes() {
                        return Err(self.schema_error_at(
                            self.program.fields[slot].tag,
                            entry.key_offset,
                            "protobuf map keys are not strictly ascending",
                        ));
                    }
                }
                self.previous_map_keys[slot] = Some(entry.key.clone());
                self.semantic_slots[slot].push(SemanticOccurrence {
                    variant: None,
                    body: None,
                    value: SemanticValue::Map(Box::new(entry)),
                    field_offset,
                });
                Ok(())
            }
            (
                Some(MessageWait::EPathMap { slot, field_offset }),
                FrameSummary::EPathMap(summary),
            ) => {
                self.semantic_slots[slot].push(SemanticOccurrence {
                    variant: None,
                    body: Some(summary.snapshot.clone()),
                    value: SemanticValue::EPathMap(Box::new(summary)),
                    field_offset,
                });
                Ok(())
            }
            (Some(_), _) => Err(self.schema_error(
                0,
                "protobuf admission child returned the wrong summary kind",
            )),
            (None, _) => Err(self.schema_error(
                0,
                "protobuf admission child returned without a continuation",
            )),
        }
    }

    fn finish(
        &mut self,
        scores: &mut CanonicalScoreArena,
    ) -> Result<MessageSummary, CanonicalProtobufAdmissionError> {
        if self.waiting.is_some() {
            return Err(self.schema_error(0, "protobuf message completed with a pending child"));
        }
        for (slot, field) in self.program.fields.iter().enumerate() {
            if matches!(field.kind, RhoMatchFieldKind::PathMap) && !self.seen[slot] {
                return Err(
                    self.schema_error(field.tag, "canonical EPathMap snapshot field is absent")
                );
            }
        }
        reduce_message(self, scores)
    }
}

fn semantic_slot<'a>(
    frame: &'a MessageFrame,
    name: &'static str,
) -> Result<&'a [SemanticOccurrence], CanonicalProtobufAdmissionError> {
    let slot = frame
        .program
        .fields
        .iter()
        .position(|field| field.name == name)
        .ok_or_else(|| frame.schema_error(0, "semantic reducer field is absent from its schema"))?;
    Ok(&frame.semantic_slots[slot])
}

fn sole_occurrence<'a>(
    frame: &'a MessageFrame,
    name: &'static str,
) -> Result<Option<&'a SemanticOccurrence>, CanonicalProtobufAdmissionError> {
    let values = semantic_slot(frame, name)?;
    if values.len() > 1 {
        return Err(frame.schema_error(0, "semantic singular field has multiple occurrences"));
    }
    Ok(values.first())
}

fn scalar_value<'a>(
    frame: &'a MessageFrame,
    name: &'static str,
) -> Result<Option<(&'a SemanticScalar, Option<&'static str>)>, CanonicalProtobufAdmissionError> {
    let Some(occurrence) = sole_occurrence(frame, name)? else {
        return Ok(None);
    };
    let SemanticValue::Scalar(value) = &occurrence.value else {
        return Err(frame.schema_error(0, "semantic scalar field returned a message summary"));
    };
    Ok(Some((value, occurrence.variant)))
}

fn bool_value(
    frame: &MessageFrame,
    name: &'static str,
) -> Result<bool, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(false),
        Some((SemanticScalar::Bool(value), _)) => Ok(*value),
        Some(_) => Err(frame.schema_error(0, "semantic bool field has a different scalar type")),
    }
}

fn i32_value(
    frame: &MessageFrame,
    name: &'static str,
) -> Result<i32, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(0),
        Some((SemanticScalar::I32(value), _)) => Ok(*value),
        Some(_) => Err(frame.schema_error(0, "semantic i32 field has a different scalar type")),
    }
}

fn u32_value(
    frame: &MessageFrame,
    name: &'static str,
) -> Result<u32, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(0),
        Some((SemanticScalar::U32(value), _)) => Ok(*value),
        Some(_) => Err(frame.schema_error(0, "semantic u32 field has a different scalar type")),
    }
}

fn bytes_value(
    frame: &MessageFrame,
    name: &'static str,
) -> Result<Option<ProtobufRegion>, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(None),
        Some((SemanticScalar::Bytes(value), _)) => Ok(Some(value.clone())),
        Some(_) => Err(frame.schema_error(0, "semantic bytes field has a different scalar type")),
    }
}

fn string_value(
    frame: &MessageFrame,
    name: &'static str,
) -> Result<Option<ProtobufRegion>, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(None),
        Some((SemanticScalar::String(value), _)) => Ok(Some(value.clone())),
        Some(_) => Err(frame.schema_error(0, "semantic string field has a different scalar type")),
    }
}

fn message_summary<'a>(
    occurrence: &'a SemanticOccurrence,
    frame: &MessageFrame,
) -> Result<&'a MessageSummary, CanonicalProtobufAdmissionError> {
    let SemanticValue::Message(summary) = &occurrence.value else {
        return Err(frame.schema_error(0, "semantic message field returned a scalar summary"));
    };
    Ok(summary)
}

fn par_occurrence<'a>(
    occurrence: &'a SemanticOccurrence,
    frame: &MessageFrame,
) -> Result<(&'a ParSummary, &'a ProtobufRegion), CanonicalProtobufAdmissionError> {
    let summary = message_summary(occurrence, frame)?;
    let SemanticSummary::Par(par) = &summary.semantic else {
        return Err(frame.schema_error(0, "semantic Par field returned a different message kind"));
    };
    let body = occurrence
        .body
        .as_ref()
        .ok_or_else(|| frame.schema_error(0, "semantic Par occurrence lost its wire body"))?;
    Ok((par, body))
}

fn optional_par<'a>(
    frame: &'a MessageFrame,
    name: &'static str,
) -> Result<Option<(&'a ParSummary, &'a ProtobufRegion)>, CanonicalProtobufAdmissionError> {
    sole_occurrence(frame, name)?
        .map(|occurrence| par_occurrence(occurrence, frame))
        .transpose()
}

fn optional_var(
    frame: &MessageFrame,
    name: &'static str,
) -> Result<Option<CanonicalScoreHandle>, CanonicalProtobufAdmissionError> {
    let Some(occurrence) = sole_occurrence(frame, name)? else {
        return Ok(None);
    };
    let summary = message_summary(occurrence, frame)?;
    let SemanticSummary::Var(score) = summary.semantic else {
        return Err(frame.schema_error(0, "semantic Var field returned a different message kind"));
    };
    Ok(Some(score))
}

fn empty_par_score(
    scores: &mut CanonicalScoreArena,
) -> Result<CanonicalScoreHandle, CanonicalProtobufAdmissionError> {
    Ok(semantics::par_score(scores, Vec::new(), false)?)
}

fn nonsemantic_score(
    scores: &mut CanonicalScoreArena,
) -> Result<CanonicalScoreHandle, CanonicalProtobufAdmissionError> {
    Ok(semantics::nonsemantic_score(scores)?)
}

fn reduce_message(
    frame: &mut MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<MessageSummary, CanonicalProtobufAdmissionError> {
    let kind = semantic_kind(frame.program)
        .ok_or_else(|| frame.schema_error(0, "message has no canonical semantic reducer"))?;
    let semantic = reduce_semantic_kind(frame, scores, kind)?;
    Ok(MessageSummary { semantic })
}

fn insert_semantic_par_identity(seen: &mut PathMap<()>, par: &ParSummary) -> bool {
    super::canonical_par_identity::replace_semantic_par_identity(
        seen,
        par.identity_prefix
            .as_bytes()
            .iter()
            .chain(par.identity_suffix.as_bytes())
            .copied(),
        (),
    )
    .is_none()
}

fn reduce_semantic_kind(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
    kind: SemanticKind,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    match kind {
        SemanticKind::RawPar => reduce_raw_par(frame, scores),
        SemanticKind::Component(family) => reduce_component(frame, scores, family),
        SemanticKind::Var => reduce_var(frame, scores),
        SemanticKind::Unit => Ok(SemanticSummary::Unit),
        SemanticKind::ReceiveBind => reduce_receive_bind(frame, scores),
        SemanticKind::MatchCase => reduce_match_case(frame, scores),
        SemanticKind::Expr => reduce_expr(frame, scores),
        SemanticKind::BigRational => {
            let numerator =
                bytes_value(frame, "numerator")?.unwrap_or_else(|| empty_region(&frame.body));
            let denominator =
                bytes_value(frame, "denominator")?.unwrap_or_else(|| empty_region(&frame.body));
            Ok(SemanticSummary::BigRational {
                numerator,
                denominator,
            })
        }
        SemanticKind::FixedPoint => Ok(SemanticSummary::FixedPoint {
            unscaled: bytes_value(frame, "unscaled")?.unwrap_or_else(|| empty_region(&frame.body)),
            scale: u32_value(frame, "scale")?,
        }),
        SemanticKind::OrderedCollection {
            constructor,
            has_remainder,
            canonical_set,
        } => reduce_ordered_collection(frame, scores, constructor, has_remainder, canonical_set),
        SemanticKind::EMap => reduce_emap(frame, scores),
        SemanticKind::EPathMap => reduce_epathmap(frame, scores),
        SemanticKind::EZipper => reduce_ezipper(frame, scores),
        SemanticKind::EMethod => reduce_emethod(frame, scores),
        SemanticKind::KeyValuePair => reduce_key_value_pair(frame),
        SemanticKind::ExprVar => reduce_expr_var(frame, scores),
        SemanticKind::ExprUnary(constructor) => reduce_expr_unary(frame, scores, constructor),
        SemanticKind::ExprBinary(constructor) => reduce_expr_binary(frame, scores, constructor),
        SemanticKind::Connective => reduce_connective(frame, scores),
        SemanticKind::VarRef => Ok(SemanticSummary::VarRef {
            index: i32_value(frame, "index")?,
            depth: i32_value(frame, "depth")?,
        }),
        SemanticKind::ConnectiveBody => {
            let mut pars = Vec::new();
            for occurrence in semantic_slot(frame, "ps")? {
                pars.push(par_occurrence(occurrence, frame)?.0.clone());
            }
            Ok(SemanticSummary::ConnectiveBody(pars))
        }
        SemanticKind::GUnforgeable => reduce_unforgeable(frame, scores),
        SemanticKind::BytesLeaf => {
            let mut value = None;
            for field in frame.program.fields {
                if let Some(bytes) = bytes_value(frame, field.name)? {
                    value = Some(bytes);
                    break;
                }
            }
            let value = value.unwrap_or_else(|| empty_region(&frame.body));
            Ok(SemanticSummary::Bytes(value))
        }
    }
}

fn empty_region(region: &ProtobufRegion) -> ProtobufRegion {
    region
        .subregion(super::protobuf_cursor::CanonicalByteRange::default())
        .expect("an empty subregion is inside every protobuf region")
}

fn occurrence_scalar<'a>(
    frame: &MessageFrame,
    occurrence: &'a SemanticOccurrence,
) -> Result<&'a SemanticScalar, CanonicalProtobufAdmissionError> {
    let SemanticValue::Scalar(value) = &occurrence.value else {
        return Err(frame.schema_error(0, "oneof scalar arm returned a message summary"));
    };
    Ok(value)
}

fn reduce_var(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let score = match sole_occurrence(frame, "var_instance")? {
        None => semantics::VarScore::Absent,
        Some(occurrence) => match occurrence.variant {
            Some("BoundVar") => {
                let SemanticScalar::I32(level) = occurrence_scalar(frame, occurrence)? else {
                    return Err(frame.schema_error(0, "BoundVar is not an i32"));
                };
                semantics::VarScore::Bound(*level)
            }
            Some("FreeVar") => {
                let SemanticScalar::I32(level) = occurrence_scalar(frame, occurrence)? else {
                    return Err(frame.schema_error(0, "FreeVar is not an i32"));
                };
                semantics::VarScore::Free(*level)
            }
            Some("Wildcard") => semantics::VarScore::Wildcard,
            _ => return Err(frame.schema_error(0, "unknown Var oneof arm")),
        },
    };
    Ok(SemanticSummary::Var(semantics::var_score(scores, score)?))
}

fn reduce_receive_bind(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let source = optional_par(frame, "source")?;
    let source_present = source.is_some();
    let score = if let Some((source, _)) = source {
        let mut patterns = Vec::new();
        for occurrence in semantic_slot(frame, "patterns")? {
            patterns.push(par_occurrence(occurrence, frame)?.0.score);
        }
        let remainder =
            optional_var(frame, "remainder")?.unwrap_or(scores.integer(i64::from(Score::ABSENT))?);
        Some(semantics::receive_bind_score(
            scores,
            source.score,
            patterns,
            remainder,
        )?)
    } else {
        None
    };
    Ok(SemanticSummary::ReceiveBind {
        score,
        source_present,
    })
}

fn reduce_match_case(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let pattern = optional_par(frame, "pattern")?;
    let source = optional_par(frame, "source")?;
    let guard = optional_par(frame, "guard")?;
    let required_present = pattern.is_some() && source.is_some();
    let canonical_fixed_point = !guard.is_some_and(|(par, _)| par.semantic_default);
    let score = match (pattern, source) {
        (Some((pattern, _)), Some((source, _))) => {
            let guard_score = guard
                .map(|(guard, _)| guard.score)
                .unwrap_or(empty_par_score(scores)?);
            Some(semantics::match_case_score(
                scores,
                pattern.score,
                source.score,
                i32_value(frame, "free_count")?,
                guard_score,
            )?)
        }
        _ => None,
    };
    Ok(SemanticSummary::MatchCase {
        score,
        required_present,
        canonical_fixed_point,
    })
}

fn reduce_expr_var(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let score = optional_var(frame, "v")?
        .map(|var| semantics::unary_score(scores, Score::EVAR, var))
        .transpose()?;
    Ok(SemanticSummary::ExprPayload(ExprPayloadSummary {
        score,
        canonical_path_stable: false,
        canonical_fixed_point: true,
        pathmap_snapshot: None,
    }))
}

fn reduce_expr_unary(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
    constructor: i32,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let score = optional_par(frame, "p")?
        .map(|(par, _)| semantics::unary_score(scores, constructor, par.score))
        .transpose()?;
    Ok(SemanticSummary::ExprPayload(ExprPayloadSummary {
        score,
        canonical_path_stable: false,
        canonical_fixed_point: true,
        pathmap_snapshot: None,
    }))
}

fn reduce_expr_binary(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
    constructor: i32,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let first_name = if frame.program.type_name == "EMatches" {
        "target"
    } else {
        "p1"
    };
    let second_name = if frame.program.type_name == "EMatches" {
        "pattern"
    } else {
        "p2"
    };
    let first = optional_par(frame, first_name)?;
    let second = optional_par(frame, second_name)?;
    let score = match (first, second) {
        (Some((first, _)), Some((second, _))) => Some(semantics::binary_score(
            scores,
            constructor,
            first.score,
            second.score,
        )?),
        _ => None,
    };
    Ok(SemanticSummary::ExprPayload(ExprPayloadSummary {
        score,
        canonical_path_stable: false,
        canonical_fixed_point: true,
        pathmap_snapshot: None,
    }))
}

fn reduce_key_value_pair(
    frame: &MessageFrame,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    Ok(SemanticSummary::KeyValuePair {
        key: optional_par(frame, "key")?.map(|(par, _)| par.clone()),
        value: optional_par(frame, "value")?.map(|(par, _)| par.clone()),
    })
}

fn reduce_raw_par(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let mut family_scores = Vec::new();
    let mut only_component = None;
    for family in semantics::PAR_SCORE_TAGS {
        let slot = frame
            .program
            .fields
            .iter()
            .position(|field| field.tag == u32::from(family.byte()))
            .ok_or_else(|| frame.schema_error(0, "raw Par family is absent from its schema"))?;
        for occurrence in &frame.semantic_slots[slot] {
            let summary = message_summary(occurrence, frame)?;
            let SemanticSummary::Component(component) = &summary.semantic else {
                return Err(frame.schema_error_at(
                    u32::from(family.byte()),
                    occurrence.field_offset,
                    "raw Par family returned a different semantic component",
                ));
            };
            if component.family != family {
                return Err(frame.schema_error_at(
                    u32::from(family.byte()),
                    occurrence.field_offset,
                    "raw Par component family disagrees with its field",
                ));
            }
            family_scores.push(component.score);
            only_component = Some(component);
        }
    }

    let connective_used = bool_value(frame, "connective_used")?;
    let score = semantics::par_score(scores, family_scores, connective_used)?;
    let locally_free_empty = semantic_slot(frame, "locally_free")?.is_empty();
    let canonical_path_stable = frame.par_components == 1
        && locally_free_empty
        && !connective_used
        && only_component.is_some_and(|component| component.canonical_path_stable);

    let body_len = frame.body.len();
    let (prefix_range, suffix_range) = match &frame.par_locally_free_range {
        Some(range) => (
            super::protobuf_cursor::CanonicalByteRange {
                start: 0,
                end: range.start,
            },
            super::protobuf_cursor::CanonicalByteRange {
                start: range.end,
                end: body_len,
            },
        ),
        None => (
            super::protobuf_cursor::CanonicalByteRange {
                start: 0,
                end: body_len,
            },
            super::protobuf_cursor::CanonicalByteRange {
                start: body_len,
                end: body_len,
            },
        ),
    };
    let identity_prefix = frame
        .body
        .subregion(prefix_range)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let identity_suffix = frame
        .body
        .subregion(suffix_range)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;

    Ok(SemanticSummary::Par(ParSummary {
        score,
        semantic_default: semantics::par_is_semantic_default(
            frame.par_components != 0,
            connective_used,
        ),
        canonical_path_stable,
        identity_prefix,
        identity_suffix,
    }))
}

fn component_summary(
    family: ParComponentTag,
    score: CanonicalScoreHandle,
    complete: bool,
    canonical_path_stable: bool,
) -> SemanticSummary {
    SemanticSummary::Component(ComponentSummary {
        family,
        score,
        complete,
        canonical_path_stable,
    })
}

fn fixed_point_error(
    frame: &MessageFrame,
    detail: &'static str,
) -> CanonicalProtobufAdmissionError {
    frame.schema_error_at(0, frame.region_offset, detail)
}

fn reduce_component(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
    family: ParComponentTag,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let (complete, score) = match family {
        ParComponentTag::Send => {
            let channel = optional_par(frame, "chan")?;
            let complete = channel.is_some();
            if !complete {
                (false, nonsemantic_score(scores)?)
            } else {
                let mut data = Vec::new();
                for occurrence in semantic_slot(frame, "data")? {
                    data.push(par_occurrence(occurrence, frame)?.0.score);
                }
                (
                    true,
                    semantics::send_score(
                        scores,
                        bool_value(frame, "persistent")?,
                        channel.expect("complete Send has a channel").0.score,
                        data,
                        bool_value(frame, "connective_used")?,
                    )?,
                )
            }
        }
        ParComponentTag::Receive => {
            let body = optional_par(frame, "body")?;
            let mut binds = Vec::new();
            let mut all_sources = true;
            for occurrence in semantic_slot(frame, "binds")? {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::ReceiveBind {
                    score,
                    source_present,
                } = &summary.semantic
                else {
                    return Err(frame.schema_error_at(
                        1,
                        occurrence.field_offset,
                        "Receive bind returned a different semantic summary",
                    ));
                };
                all_sources &= *source_present;
                if let Some(score) = score {
                    binds.push(*score);
                }
            }
            let complete = body.is_some() && all_sources;
            if !complete {
                (false, nonsemantic_score(scores)?)
            } else {
                let condition = optional_par(frame, "condition")?;
                if condition.is_some_and(|(par, _)| par.semantic_default) {
                    return Err(fixed_point_error(
                        frame,
                        "Receive condition is a present semantic default",
                    ));
                }
                let condition_score = match condition {
                    Some((condition, _)) => condition.score,
                    None => empty_par_score(scores)?,
                };
                (
                    true,
                    semantics::receive_score(
                        scores,
                        bool_value(frame, "persistent")?,
                        bool_value(frame, "peek")?,
                        binds,
                        body.expect("complete Receive has a body").0.score,
                        i32_value(frame, "bind_count")?,
                        bool_value(frame, "connective_used")?,
                        condition_score,
                    )?,
                )
            }
        }
        ParComponentTag::New => {
            let body = optional_par(frame, "p")?;
            if body.is_none() {
                (false, nonsemantic_score(scores)?)
            } else {
                let uris = semantic_slot(frame, "uri")?;
                let mut uri_scores = Vec::new();
                let mut previous_uri: Option<&str> = None;
                for occurrence in uris {
                    let SemanticValue::Scalar(SemanticScalar::String(uri)) = &occurrence.value
                    else {
                        return Err(frame.schema_error_at(
                            3,
                            occurrence.field_offset,
                            "New URI returned a different semantic scalar",
                        ));
                    };
                    let uri = std::str::from_utf8(uri.as_bytes())
                        .expect("protobuf string validation precedes semantic reduction");
                    if previous_uri.is_some_and(|previous| previous > uri) {
                        return Err(frame.schema_error_at(
                            3,
                            occurrence.field_offset,
                            "New URIs are outside canonical string order",
                        ));
                    }
                    previous_uri = Some(uri);
                    uri_scores.push(scores.string(uri)?);
                }
                if uri_scores.is_empty() {
                    uri_scores.push(scores.integer(i64::from(Score::ABSENT))?);
                }

                let mut injection_scores = Vec::new();
                for occurrence in semantic_slot(frame, "injections")? {
                    let SemanticValue::Map(entry) = &occurrence.value else {
                        return Err(frame.schema_error_at(
                            4,
                            occurrence.field_offset,
                            "New injection returned a different semantic summary",
                        ));
                    };
                    let key = std::str::from_utf8(entry.key.as_bytes())
                        .expect("protobuf map-key validation precedes semantic reduction");
                    let value_score = match &entry.value {
                        Some(value) => value.score,
                        None => empty_par_score(scores)?,
                    };
                    injection_scores.push(semantics::injection_score(scores, key, value_score)?);
                }
                if injection_scores.is_empty() {
                    injection_scores.push(scores.integer(i64::from(Score::ABSENT))?);
                }
                (
                    true,
                    semantics::new_score(
                        scores,
                        i32_value(frame, "bind_count")?,
                        uri_scores,
                        injection_scores,
                        body.expect("complete New has a body").0.score,
                    )?,
                )
            }
        }
        ParComponentTag::Match => {
            let target = optional_par(frame, "target")?;
            let mut cases = Vec::new();
            let mut all_required = true;
            let mut all_fixed = true;
            for occurrence in semantic_slot(frame, "cases")? {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::MatchCase {
                    score,
                    required_present,
                    canonical_fixed_point,
                } = &summary.semantic
                else {
                    return Err(frame.schema_error_at(
                        2,
                        occurrence.field_offset,
                        "Match case returned a different semantic summary",
                    ));
                };
                all_required &= *required_present;
                all_fixed &= *canonical_fixed_point;
                if let Some(score) = score {
                    cases.push(*score);
                }
            }
            let complete = target.is_some() && all_required;
            if !complete {
                (false, nonsemantic_score(scores)?)
            } else {
                if !all_fixed {
                    return Err(fixed_point_error(
                        frame,
                        "Match guard is a present semantic default",
                    ));
                }
                (
                    true,
                    semantics::match_score(
                        scores,
                        target.expect("complete Match has a target").0.score,
                        cases,
                        bool_value(frame, "connective_used")?,
                    )?,
                )
            }
        }
        ParComponentTag::Bundle => {
            let body = optional_par(frame, "body")?;
            if let Some((body, _)) = body {
                (
                    true,
                    semantics::bundle_score(
                        scores,
                        bool_value(frame, "read_flag")?,
                        bool_value(frame, "write_flag")?,
                        body.score,
                    )?,
                )
            } else {
                (false, nonsemantic_score(scores)?)
            }
        }
        ParComponentTag::If => {
            let condition = optional_par(frame, "condition")?;
            let if_true = optional_par(frame, "if_true")?;
            let if_false = optional_par(frame, "if_false")?;
            if let (Some((condition, _)), Some((if_true, _)), Some((if_false, _))) =
                (condition, if_true, if_false)
            {
                (
                    true,
                    semantics::if_score(
                        scores,
                        condition.score,
                        if_true.score,
                        if_false.score,
                        bool_value(frame, "connective_used")?,
                    )?,
                )
            } else {
                (false, nonsemantic_score(scores)?)
            }
        }
        ParComponentTag::Expr | ParComponentTag::Connective | ParComponentTag::GUnforgeable => {
            return Err(frame.schema_error(
                u32::from(family.byte()),
                "component reducer received a specialized component family",
            ));
        }
    };
    Ok(component_summary(family, score, complete, false))
}

fn reduce_expr(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let occurrence = sole_occurrence(frame, "expr_instance")?;
    let (score, complete, stable, fixed) = match occurrence {
        None => (
            semantics::ground_score(scores, semantics::GroundScore::AbsentExpr)?,
            true,
            false,
            true,
        ),
        Some(occurrence) => match (&occurrence.variant, &occurrence.value) {
            (Some("GBool"), SemanticValue::Scalar(SemanticScalar::Bool(value))) => (
                semantics::ground_score(scores, semantics::GroundScore::Bool(*value))?,
                true,
                true,
                true,
            ),
            (Some("GInt"), SemanticValue::Scalar(SemanticScalar::I64(value))) => (
                semantics::ground_score(scores, semantics::GroundScore::Int(*value))?,
                true,
                true,
                true,
            ),
            (Some("GString"), SemanticValue::Scalar(SemanticScalar::String(value))) => {
                let value = std::str::from_utf8(value.as_bytes())
                    .expect("protobuf string validation precedes semantic reduction");
                (
                    semantics::ground_score(scores, semantics::GroundScore::String(value))?,
                    true,
                    true,
                    true,
                )
            }
            (Some("GUri"), SemanticValue::Scalar(SemanticScalar::String(value))) => {
                let value = std::str::from_utf8(value.as_bytes())
                    .expect("protobuf string validation precedes semantic reduction");
                (
                    semantics::ground_score(scores, semantics::GroundScore::Uri(value))?,
                    true,
                    true,
                    true,
                )
            }
            (Some("GByteArray"), SemanticValue::Scalar(SemanticScalar::Bytes(value))) => (
                semantics::ground_score(
                    scores,
                    semantics::GroundScore::ByteArray(value.as_bytes()),
                )?,
                true,
                true,
                true,
            ),
            (Some("GDouble"), SemanticValue::Scalar(SemanticScalar::U64(value))) => (
                semantics::ground_score(scores, semantics::GroundScore::Double(*value))?,
                true,
                true,
                true,
            ),
            (Some("GBigInt"), SemanticValue::Scalar(SemanticScalar::Bytes(value))) => (
                semantics::ground_score(scores, semantics::GroundScore::BigInt(value.as_bytes()))?,
                true,
                true,
                true,
            ),
            (Some("GBigRat"), SemanticValue::Message(summary)) => {
                let SemanticSummary::BigRational {
                    numerator,
                    denominator,
                } = &summary.semantic
                else {
                    return Err(frame.schema_error_at(
                        36,
                        occurrence.field_offset,
                        "GBigRat returned a different semantic summary",
                    ));
                };
                (
                    semantics::ground_score(scores, semantics::GroundScore::BigRat {
                        numerator: numerator.as_bytes(),
                        denominator: denominator.as_bytes(),
                    })?,
                    true,
                    true,
                    true,
                )
            }
            (Some("GFixedPoint"), SemanticValue::Message(summary)) => {
                let SemanticSummary::FixedPoint { unscaled, scale } = &summary.semantic else {
                    return Err(frame.schema_error_at(
                        37,
                        occurrence.field_offset,
                        "GFixedPoint returned a different semantic summary",
                    ));
                };
                (
                    semantics::ground_score(scores, semantics::GroundScore::FixedPoint {
                        unscaled: unscaled.as_bytes(),
                        scale: *scale,
                    })?,
                    true,
                    true,
                    true,
                )
            }
            (Some(_), SemanticValue::Message(summary)) => {
                let SemanticSummary::ExprPayload(payload) = &summary.semantic else {
                    return Err(frame.schema_error_at(
                        1,
                        occurrence.field_offset,
                        "Expr payload returned a different semantic summary",
                    ));
                };
                match payload.score {
                    Some(score) => (
                        score,
                        true,
                        payload.canonical_path_stable,
                        payload.canonical_fixed_point,
                    ),
                    None => (nonsemantic_score(scores)?, false, false, true),
                }
            }
            _ => {
                return Err(frame.schema_error_at(
                    1,
                    occurrence.field_offset,
                    "Expr oneof arm and semantic payload disagree",
                ));
            }
        },
    };
    if complete && !fixed {
        return Err(fixed_point_error(
            frame,
            "complete Expr body is outside the canonical sorter fixed point",
        ));
    }
    Ok(component_summary(
        ParComponentTag::Expr,
        score,
        complete,
        stable,
    ))
}

fn reduce_connective(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let score = match sole_occurrence(frame, "connective_instance")? {
        None => semantics::connective_score(scores, semantics::ConnectiveScore::Absent)?,
        Some(occurrence) => match occurrence.variant {
            Some("ConnAndBody") | Some("ConnOrBody") => {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::ConnectiveBody(pars) = &summary.semantic else {
                    return Err(frame.schema_error_at(
                        1,
                        occurrence.field_offset,
                        "connective collection returned a different semantic summary",
                    ));
                };
                let children = pars.iter().map(|par| par.score).collect();
                if occurrence.variant == Some("ConnAndBody") {
                    semantics::connective_score(scores, semantics::ConnectiveScore::And(children))?
                } else {
                    semantics::connective_score(scores, semantics::ConnectiveScore::Or(children))?
                }
            }
            Some("ConnNotBody") => {
                let (par, _) = par_occurrence(occurrence, frame)?;
                semantics::connective_score(scores, semantics::ConnectiveScore::Not(par.score))?
            }
            Some("VarRefBody") => {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::VarRef { index, depth } = summary.semantic else {
                    return Err(frame.schema_error_at(
                        4,
                        occurrence.field_offset,
                        "VarRef returned a different semantic summary",
                    ));
                };
                semantics::connective_score(scores, semantics::ConnectiveScore::VarRef {
                    index,
                    depth,
                })?
            }
            Some("ConnBool")
            | Some("ConnInt")
            | Some("ConnString")
            | Some("ConnUri")
            | Some("ConnByteArray") => {
                let SemanticScalar::Bool(value) = occurrence_scalar(frame, occurrence)? else {
                    return Err(frame.schema_error_at(
                        1,
                        occurrence.field_offset,
                        "connective scalar arm is not boolean",
                    ));
                };
                let connective = match occurrence.variant {
                    Some("ConnBool") => semantics::ConnectiveScore::Bool(*value),
                    Some("ConnInt") => semantics::ConnectiveScore::Int(*value),
                    Some("ConnString") => semantics::ConnectiveScore::String(*value),
                    Some("ConnUri") => semantics::ConnectiveScore::Uri(*value),
                    Some("ConnByteArray") => semantics::ConnectiveScore::ByteArray(*value),
                    _ => unreachable!("the scalar connective variants were matched above"),
                };
                semantics::connective_score(scores, connective)?
            }
            _ => return Err(frame.schema_error(1, "unknown Connective oneof arm")),
        },
    };
    Ok(component_summary(
        ParComponentTag::Connective,
        score,
        true,
        false,
    ))
}

fn reduce_unforgeable(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let (descriptor, stable) = match sole_occurrence(frame, "unf_instance")? {
        None => (semantics::UnforgeableScore::Absent, false),
        Some(occurrence) => match occurrence.variant {
            Some("GPrivateBody") => {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::Bytes(value) = &summary.semantic else {
                    return Err(frame.schema_error_at(
                        1,
                        occurrence.field_offset,
                        "GPrivate returned a different semantic summary",
                    ));
                };
                (semantics::UnforgeableScore::Private(value.as_bytes()), true)
            }
            Some("GDeployIdBody") => {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::Bytes(value) = &summary.semantic else {
                    return Err(frame.schema_error_at(
                        2,
                        occurrence.field_offset,
                        "GDeployId returned a different semantic summary",
                    ));
                };
                (
                    semantics::UnforgeableScore::DeployId(value.as_bytes()),
                    false,
                )
            }
            Some("GDeployerIdBody") => {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::Bytes(value) = &summary.semantic else {
                    return Err(frame.schema_error_at(
                        3,
                        occurrence.field_offset,
                        "GDeployerId returned a different semantic summary",
                    ));
                };
                (
                    semantics::UnforgeableScore::DeployerId(value.as_bytes()),
                    false,
                )
            }
            Some("GSysAuthTokenBody") => (semantics::UnforgeableScore::SysAuthToken, false),
            _ => return Err(frame.schema_error(1, "unknown GUnforgeable oneof arm")),
        },
    };
    let score = semantics::unforgeable_score(scores, descriptor)?;
    Ok(component_summary(
        ParComponentTag::GUnforgeable,
        score,
        true,
        stable,
    ))
}

fn reduce_ordered_collection(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
    constructor: i32,
    has_remainder: bool,
    canonical_set: bool,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let occurrences = semantic_slot(frame, "ps")?;
    let mut element_scores = Vec::with_capacity(occurrences.len());
    let mut all_stable = true;
    let mut canonical_fixed_point = true;
    let mut previous: Option<(&ParSummary, &ProtobufRegion)> = None;
    let mut identities = canonical_set.then(PathMap::new);
    for occurrence in occurrences {
        let (par, body) = par_occurrence(occurrence, frame)?;
        element_scores.push(par.score);
        all_stable &= par.canonical_path_stable;
        if canonical_set {
            if let Some((previous_par, previous_body)) = previous {
                let order = scores
                    .compare(previous_par.score, par.score)
                    .then_with(|| previous_body.as_bytes().cmp(body.as_bytes()));
                canonical_fixed_point &= order.is_lt();
            }
            canonical_fixed_point &= insert_semantic_par_identity(
                identities
                    .as_mut()
                    .expect("canonical set reduction owns an identity trie"),
                par,
            );
            previous = Some((par, body));
        }
    }

    let remainder = if has_remainder {
        Some(optional_var(frame, "remainder")?.unwrap_or(scores.integer(-1)?))
    } else {
        None
    };
    let connective_used = bool_value(frame, "connective_used")?;
    let score = semantics::ordered_collection_score(
        scores,
        constructor,
        remainder,
        element_scores,
        connective_used,
    )?;
    let stable = !canonical_set
        && semantic_slot(frame, "locally_free")?.is_empty()
        && !connective_used
        && (!has_remainder || optional_var(frame, "remainder")?.is_none())
        && all_stable;
    Ok(SemanticSummary::ExprPayload(ExprPayloadSummary {
        score: Some(score),
        canonical_path_stable: stable,
        canonical_fixed_point,
        pathmap_snapshot: None,
    }))
}

fn reduce_emap(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let occurrences = semantic_slot(frame, "kvs")?;
    let mut pair_scores = Vec::with_capacity(occurrences.len());
    let mut complete = true;
    let mut canonical_fixed_point = true;
    let mut previous: Option<(&ParSummary, &ProtobufRegion)> = None;
    let mut identities = PathMap::new();

    for occurrence in occurrences {
        let summary = message_summary(occurrence, frame)?;
        let SemanticSummary::KeyValuePair { key, value } = &summary.semantic else {
            return Err(frame.schema_error(0, "EMap pair returned the wrong semantic summary"));
        };
        let Some(key) = key else {
            complete = false;
            continue;
        };
        if value.is_none() {
            complete = false;
            continue;
        }
        let pair_body = occurrence
            .body
            .as_ref()
            .ok_or_else(|| frame.schema_error(0, "EMap pair occurrence lost its protobuf body"))?;
        if let Some((previous_key, previous_pair_body)) = previous {
            let order = scores
                .compare(previous_key.score, key.score)
                .then_with(|| previous_pair_body.as_bytes().cmp(pair_body.as_bytes()));
            canonical_fixed_point &= order.is_lt();
        }
        canonical_fixed_point &= insert_semantic_par_identity(&mut identities, key);
        previous = Some((key, pair_body));
        pair_scores.push(key.score);
    }

    let score = if complete {
        let remainder = optional_var(frame, "remainder")?.unwrap_or(scores.integer(-1)?);
        Some(semantics::ordered_collection_score(
            scores,
            Score::EMAP,
            Some(remainder),
            pair_scores,
            bool_value(frame, "connective_used")?,
        )?)
    } else {
        None
    };
    Ok(SemanticSummary::ExprPayload(ExprPayloadSummary {
        score,
        canonical_path_stable: false,
        canonical_fixed_point,
        pathmap_snapshot: None,
    }))
}

fn reduce_epathmap(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let occurrence = sole_occurrence(frame, "entries")?
        .ok_or_else(|| frame.schema_error(9, "EPathMap snapshot semantic summary is absent"))?;
    let SemanticValue::EPathMap(snapshot) = &occurrence.value else {
        return Err(frame.schema_error(9, "EPathMap snapshot returned the wrong semantic summary"));
    };
    let snapshot_score = scores.shared_region(snapshot.snapshot.clone())?;
    let score = semantics::rpb3_pathmap_score(
        scores,
        false,
        snapshot_score,
        bool_value(frame, "connective_used")?,
    )?;
    Ok(SemanticSummary::ExprPayload(ExprPayloadSummary {
        score: Some(score),
        canonical_path_stable: semantic_slot(frame, "locally_free")?.is_empty()
            && !bool_value(frame, "connective_used")?
            && optional_var(frame, "remainder")?.is_none()
            && snapshot.entries_canonical_path_stable,
        canonical_fixed_point: true,
        pathmap_snapshot: Some(snapshot.snapshot.clone()),
    }))
}

fn reduce_ezipper(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let Some(occurrence) = sole_occurrence(frame, "pathmap")? else {
        return Ok(SemanticSummary::ExprPayload(ExprPayloadSummary {
            score: None,
            canonical_path_stable: false,
            canonical_fixed_point: true,
            pathmap_snapshot: None,
        }));
    };
    let summary = message_summary(occurrence, frame)?;
    let SemanticSummary::ExprPayload(pathmap) = &summary.semantic else {
        return Err(frame.schema_error(0, "EZipper pathmap returned the wrong semantic summary"));
    };
    let Some(_score) = pathmap.score else {
        return Err(frame.schema_error(0, "EZipper pathmap has no canonical score"));
    };
    let snapshot = pathmap
        .pathmap_snapshot
        .as_ref()
        .ok_or_else(|| frame.schema_error(0, "EZipper pathmap summary has no EPathMap snapshot"))?;
    let snapshot_score = scores.shared_region(snapshot.clone())?;
    let zipper_score = semantics::rpb3_pathmap_score(
        scores,
        true,
        snapshot_score,
        bool_value(frame, "connective_used")?,
    )?;
    Ok(SemanticSummary::ExprPayload(ExprPayloadSummary {
        score: Some(zipper_score),
        canonical_path_stable: false,
        canonical_fixed_point: pathmap.canonical_fixed_point,
        pathmap_snapshot: None,
    }))
}

fn reduce_emethod(
    frame: &MessageFrame,
    scores: &mut CanonicalScoreArena,
) -> Result<SemanticSummary, CanonicalProtobufAdmissionError> {
    let target = optional_par(frame, "target")?;
    let score = if let Some((target, _)) = target {
        let mut arguments = Vec::new();
        for occurrence in semantic_slot(frame, "arguments")? {
            arguments.push(par_occurrence(occurrence, frame)?.0.score);
        }
        let method =
            string_value(frame, "method_name")?.unwrap_or_else(|| empty_region(&frame.body));
        let method = std::str::from_utf8(method.as_bytes())
            .expect("protobuf string validation precedes semantic reduction");
        Some(semantics::method_score(
            scores,
            method,
            target.score,
            arguments,
            bool_value(frame, "connective_used")?,
        )?)
    } else {
        None
    };
    Ok(SemanticSummary::ExprPayload(ExprPayloadSummary {
        score,
        canonical_path_stable: false,
        canonical_fixed_point: true,
        pathmap_snapshot: None,
    }))
}

struct MapFrame {
    owner: &'static RhoMatchProgram,
    outer_field: u32,
    value_type_id: u16,
    region: ProtobufRegion,
    region_offset: usize,
    cursor: CanonicalRegionFieldCursor,
    key: Option<ProtobufRegion>,
    key_offset: Option<usize>,
    saw_value: bool,
    value_offset: Option<usize>,
    value: Option<ParSummary>,
    waiting_value: bool,
}

impl MapFrame {
    fn new(
        owner: &'static RhoMatchProgram,
        outer_field: u32,
        value_type_id: u16,
        region: ProtobufRegion,
    ) -> Self {
        let region_offset = region.absolute_range().start;
        Self {
            owner,
            outer_field,
            value_type_id,
            cursor: CanonicalRegionFieldCursor::new(region.clone()),
            region,
            region_offset,
            key: None,
            key_offset: None,
            saw_value: false,
            value_offset: None,
            value: None,
            waiting_value: false,
        }
    }

    fn schema_error(&self, detail: &'static str) -> CanonicalProtobufAdmissionError {
        self.schema_error_at(self.outer_field, self.region_offset, detail)
    }

    fn schema_error_at(
        &self,
        field: u32,
        region_offset: usize,
        detail: &'static str,
    ) -> CanonicalProtobufAdmissionError {
        CanonicalProtobufAdmissionError::Schema {
            type_name: "protobuf map entry",
            field,
            region_offset,
            detail,
        }
    }

    fn field_error(
        &self,
        field: &CanonicalRegionField,
        detail: &'static str,
    ) -> CanonicalProtobufAdmissionError {
        let absolute = self
            .region_offset
            .checked_add(field.encoded_range.start)
            .expect("a map-entry field offset remains inside its protobuf backing");
        self.schema_error_at(field.number, absolute, detail)
    }

    fn accept(&mut self, summary: FrameSummary) -> Result<(), CanonicalProtobufAdmissionError> {
        if !self.waiting_value {
            return Err(self.schema_error("map value returned without a continuation"));
        }
        self.waiting_value = false;
        let FrameSummary::Message(summary) = summary else {
            return Err(self.schema_error("map value returned the wrong summary kind"));
        };
        let SemanticSummary::Par(value) = summary.semantic else {
            return Err(self.schema_error("map value did not return a raw Par summary"));
        };
        if value.semantic_default {
            return Err(self.schema_error_at(
                2,
                self.value_offset.unwrap_or(self.region_offset),
                "explicit protobuf map value equals the omitted default",
            ));
        }
        self.value = Some(value);
        Ok(())
    }

    fn finish(&mut self) -> Result<MapSummary, CanonicalProtobufAdmissionError> {
        if self.waiting_value {
            return Err(self.schema_error("map entry completed with a pending value"));
        }
        let key = self.key.clone().unwrap_or_else(|| {
            self.region
                .subregion(super::protobuf_cursor::CanonicalByteRange::default())
                .expect("an empty range is inside every map entry")
        });
        Ok(MapSummary {
            key,
            key_offset: self.key_offset.unwrap_or(self.region_offset),
            value: self.value.take(),
        })
    }
}

struct EPathMapFrame {
    owner: &'static RhoMatchProgram,
    field: u32,
    region_offset: usize,
    snapshot: ProtobufRegion,
    cursor: OwnedCanonicalEpm1TopologyCursor,
    path: Vec<u8>,
    entries_canonical_path_stable: bool,
    waiting_value: bool,
}

impl EPathMapFrame {
    fn accept(&mut self, summary: FrameSummary) -> Result<(), CanonicalProtobufAdmissionError> {
        if !self.waiting_value {
            return Err(CanonicalProtobufAdmissionError::Schema {
                type_name: self.owner.type_name,
                field: self.field,
                region_offset: self.region_offset,
                detail: "EPM1 value returned without a continuation",
            });
        }
        self.waiting_value = false;
        let FrameSummary::Message(MessageSummary {
            semantic: SemanticSummary::Par(par),
        }) = summary
        else {
            return Err(CanonicalProtobufAdmissionError::Schema {
                type_name: self.owner.type_name,
                field: self.field,
                region_offset: self.region_offset,
                detail: "EPM1 value did not return a raw Par summary",
            });
        };
        self.entries_canonical_path_stable &= par.canonical_path_stable;
        Ok(())
    }
}

enum Frame {
    Message(MessageFrame),
    Map(MapFrame),
    EPathMap(EPathMapFrame),
}

#[derive(Clone)]
struct MessageSummary {
    semantic: SemanticSummary,
}

#[derive(Clone)]
struct MapSummary {
    key: ProtobufRegion,
    key_offset: usize,
    value: Option<ParSummary>,
}

enum FrameSummary {
    Message(MessageSummary),
    Map(MapSummary),
    EPathMap(EPathMapSummary),
}

enum FrameAction {
    Continue,
    Push(Frame),
    Complete(FrameSummary),
}

fn canonical_wire_type(kind: ProtobufWireType) -> CanonicalWireType {
    match kind {
        ProtobufWireType::Varint => CanonicalWireType::Varint,
        ProtobufWireType::Fixed64 => CanonicalWireType::Fixed64,
        ProtobufWireType::LengthDelimited => CanonicalWireType::LengthDelimited,
        ProtobufWireType::Fixed32 => CanonicalWireType::Fixed32,
    }
}

fn verify_wire(
    frame: &MessageFrame,
    field: &CanonicalRegionField,
    resolved: ResolvedProtobufField,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let expected = resolved
        .protobuf_kind()
        .wire_type()
        .map(canonical_wire_type)
        .ok_or_else(|| frame.field_error(field, "unresolved oneof has no wire type"))?;
    if field.wire_type == expected {
        Ok(())
    } else {
        Err(frame.field_error(field, "protobuf field has the wrong wire type"))
    }
}

#[derive(Clone, Copy)]
enum ScalarPresence {
    Plain,
    Repeated,
    Oneof,
}

impl ScalarPresence {
    fn permits_default(self) -> bool { matches!(self, Self::Repeated | Self::Oneof) }
}

fn scalar_error(
    frame: &MessageFrame,
    field: &CanonicalRegionField,
    detail: &'static str,
) -> CanonicalProtobufAdmissionError {
    CanonicalProtobufAdmissionError::Schema {
        type_name: frame.program.type_name,
        field: field.number,
        region_offset: frame
            .region_offset
            .checked_add(field.encoded_range.start)
            .expect("a field relative offset remains inside its protobuf backing"),
        detail,
    }
}

fn fixed_is_zero(region: &ProtobufRegion) -> bool {
    region.as_bytes().iter().all(|&byte| byte == 0)
}

fn validate_scalar(
    frame: &MessageFrame,
    field: &CanonicalRegionField,
    kind: ProtobufKind,
    presence: ScalarPresence,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let default_forbidden = !presence.permits_default();
    match (kind, &field.value) {
        (ProtobufKind::Bool, CanonicalRegionFieldValue::Varint(value)) => {
            if *value > 1 {
                return Err(scalar_error(
                    frame,
                    field,
                    "protobuf bool is not zero or one",
                ));
            }
            if default_forbidden && *value == 0 {
                return Err(scalar_error(
                    frame,
                    field,
                    "explicit protobuf scalar default",
                ));
            }
        }
        (ProtobufKind::Int32, CanonicalRegionFieldValue::Varint(value)) => {
            if (*value as u32 as i32) as u64 != *value {
                return Err(scalar_error(
                    frame,
                    field,
                    "protobuf int32 has a noncanonical alias",
                ));
            }
            if default_forbidden && *value == 0 {
                return Err(scalar_error(
                    frame,
                    field,
                    "explicit protobuf scalar default",
                ));
            }
        }
        (ProtobufKind::Sint32 | ProtobufKind::Uint32, CanonicalRegionFieldValue::Varint(value)) => {
            if u32::try_from(*value).is_err() {
                return Err(scalar_error(
                    frame,
                    field,
                    "protobuf 32-bit scalar overflows u32",
                ));
            }
            if default_forbidden && *value == 0 {
                return Err(scalar_error(
                    frame,
                    field,
                    "explicit protobuf scalar default",
                ));
            }
        }
        (
            ProtobufKind::Int64 | ProtobufKind::Sint64 | ProtobufKind::Uint64,
            CanonicalRegionFieldValue::Varint(value),
        ) => {
            if default_forbidden && *value == 0 {
                return Err(scalar_error(
                    frame,
                    field,
                    "explicit protobuf scalar default",
                ));
            }
        }
        (
            ProtobufKind::Sfixed32 | ProtobufKind::Fixed32,
            CanonicalRegionFieldValue::Fixed32(value),
        )
        | (ProtobufKind::Float, CanonicalRegionFieldValue::Fixed32(value)) => {
            let zero = if kind == ProtobufKind::Float {
                let bits = u32::from_le_bytes(value.as_bytes().try_into().expect("fixed32 length"));
                bits & 0x7fff_ffff == 0
            } else {
                fixed_is_zero(value)
            };
            if default_forbidden && zero {
                return Err(scalar_error(
                    frame,
                    field,
                    "explicit protobuf scalar default",
                ));
            }
        }
        (
            ProtobufKind::Sfixed64 | ProtobufKind::Fixed64,
            CanonicalRegionFieldValue::Fixed64(value),
        )
        | (ProtobufKind::Double, CanonicalRegionFieldValue::Fixed64(value)) => {
            let zero = if kind == ProtobufKind::Double {
                let bits = u64::from_le_bytes(value.as_bytes().try_into().expect("fixed64 length"));
                bits & 0x7fff_ffff_ffff_ffff == 0
            } else {
                fixed_is_zero(value)
            };
            if default_forbidden && zero {
                return Err(scalar_error(
                    frame,
                    field,
                    "explicit protobuf scalar default",
                ));
            }
        }
        (
            ProtobufKind::String | ProtobufKind::RepeatedString,
            CanonicalRegionFieldValue::LengthDelimited(value),
        ) => {
            std::str::from_utf8(value.as_bytes())
                .map_err(|_| scalar_error(frame, field, "protobuf string is not UTF-8"))?;
            if default_forbidden && value.is_empty() {
                return Err(scalar_error(
                    frame,
                    field,
                    "explicit protobuf scalar default",
                ));
            }
        }
        (
            ProtobufKind::Bytes | ProtobufKind::RepeatedBytes,
            CanonicalRegionFieldValue::LengthDelimited(value),
        ) => {
            if default_forbidden && value.is_empty() {
                return Err(scalar_error(
                    frame,
                    field,
                    "explicit protobuf scalar default",
                ));
            }
        }
        _ => {
            return Err(scalar_error(
                frame,
                field,
                "protobuf scalar kind and wire value disagree",
            ));
        }
    }
    Ok(())
}

fn semantic_scalar(
    field: &CanonicalRegionField,
    kind: ProtobufKind,
) -> Result<SemanticScalar, CanonicalProtobufAdmissionError> {
    let scalar = match (kind, &field.value) {
        (ProtobufKind::Bool, CanonicalRegionFieldValue::Varint(value)) => {
            SemanticScalar::Bool(*value != 0)
        }
        (ProtobufKind::Int32, CanonicalRegionFieldValue::Varint(value)) => {
            SemanticScalar::I32(*value as u32 as i32)
        }
        (ProtobufKind::Sint32, CanonicalRegionFieldValue::Varint(value)) => {
            let encoded = *value as u32;
            SemanticScalar::I32(((encoded >> 1) as i32) ^ -((encoded & 1) as i32))
        }
        (ProtobufKind::Sfixed32, CanonicalRegionFieldValue::Fixed32(value)) => {
            SemanticScalar::I32(i32::from_le_bytes(
                value
                    .as_bytes()
                    .try_into()
                    .expect("a fixed32 region has four bytes"),
            ))
        }
        (
            ProtobufKind::Uint32 | ProtobufKind::Fixed32 | ProtobufKind::Float,
            CanonicalRegionFieldValue::Varint(value),
        ) if kind == ProtobufKind::Uint32 => SemanticScalar::U32(*value as u32),
        (
            ProtobufKind::Fixed32 | ProtobufKind::Float,
            CanonicalRegionFieldValue::Fixed32(value),
        ) => SemanticScalar::U32(u32::from_le_bytes(
            value
                .as_bytes()
                .try_into()
                .expect("a fixed32 region has four bytes"),
        )),
        (ProtobufKind::Int64, CanonicalRegionFieldValue::Varint(value)) => {
            SemanticScalar::I64(*value as i64)
        }
        (ProtobufKind::Sint64, CanonicalRegionFieldValue::Varint(value)) => {
            SemanticScalar::I64(((value >> 1) as i64) ^ -((value & 1) as i64))
        }
        (ProtobufKind::Sfixed64, CanonicalRegionFieldValue::Fixed64(value)) => {
            SemanticScalar::I64(i64::from_le_bytes(
                value
                    .as_bytes()
                    .try_into()
                    .expect("a fixed64 region has eight bytes"),
            ))
        }
        (ProtobufKind::Uint64, CanonicalRegionFieldValue::Varint(value)) => {
            SemanticScalar::U64(*value)
        }
        (
            ProtobufKind::Fixed64 | ProtobufKind::Double,
            CanonicalRegionFieldValue::Fixed64(value),
        ) => SemanticScalar::U64(u64::from_le_bytes(
            value
                .as_bytes()
                .try_into()
                .expect("a fixed64 region has eight bytes"),
        )),
        (
            ProtobufKind::String | ProtobufKind::RepeatedString,
            CanonicalRegionFieldValue::LengthDelimited(value),
        ) => SemanticScalar::String(value.clone()),
        (
            ProtobufKind::Bytes | ProtobufKind::RepeatedBytes,
            CanonicalRegionFieldValue::LengthDelimited(value),
        ) => SemanticScalar::Bytes(value.clone()),
        _ => return Err(CanonicalProtobufAdmissionError::SizeOverflow),
    };
    Ok(scalar)
}

fn field_absolute_offset(
    frame: &MessageFrame,
    field: &CanonicalRegionField,
) -> Result<usize, CanonicalProtobufAdmissionError> {
    frame
        .region_offset
        .checked_add(field.encoded_range.start)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)
}

fn record_scalar(
    frame: &mut MessageFrame,
    field: &CanonicalRegionField,
    slot: usize,
    variant: Option<&'static str>,
    kind: ProtobufKind,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let field_offset = field_absolute_offset(frame, field)?;
    let value = semantic_scalar(field, kind)?;
    if frame.is_raw_par() && frame.program.fields[slot].name == "locally_free" {
        frame.par_locally_free_range = Some(field.encoded_range.clone());
    }
    frame.semantic_slots[slot].push(SemanticOccurrence {
        variant,
        body: None,
        value: SemanticValue::Scalar(value),
        field_offset,
    });
    Ok(())
}

fn child_program(
    frame: &MessageFrame,
    field: &CanonicalRegionField,
    type_id: u16,
) -> Result<&'static RhoMatchProgram, CanonicalProtobufAdmissionError> {
    wire_program_by_type_id(type_id).map_err(|error| frame.field_error(field, error.detail()))
}

fn message_step(
    frame: &mut MessageFrame,
    stats: &mut CanonicalProtobufAdmissionStats,
    scores: &mut CanonicalScoreArena,
) -> Result<FrameAction, CanonicalProtobufAdmissionError> {
    if frame.waiting.is_some() {
        return Err(frame.schema_error(0, "protobuf message resumed before its child returned"));
    }
    let field =
        frame
            .cursor
            .next_field()
            .map_err(|source| CanonicalProtobufAdmissionError::Framing {
                type_name: frame.program.type_name,
                region_offset: frame.region_offset,
                source,
            })?;
    let Some(field) = field else {
        return Ok(FrameAction::Complete(FrameSummary::Message(
            frame.finish(scores)?,
        )));
    };
    stats.fields = stats
        .fields
        .checked_add(1)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;

    let resolved = resolve_protobuf_field(frame.program, field.number)
        .ok_or_else(|| frame.field_error(&field, "field is outside the generated schema"))?;
    verify_wire(frame, &field, resolved)?;
    let slot = resolved.slot();
    if frame.last_slot.is_some_and(|last| slot < last) {
        return Err(frame.field_error(&field, "field is outside canonical Prost slot order"));
    }
    if !resolved.repeated() && frame.seen[slot] {
        return Err(frame.field_error(&field, "canonical singular or oneof field is duplicated"));
    }
    frame.last_slot = Some(slot);
    frame.seen[slot] = true;
    let field_offset = field_absolute_offset(frame, &field)?;

    let schema_field = resolved.field();
    if frame.is_raw_par() && matches!(schema_field.kind, RhoMatchFieldKind::MessageSequence(_)) {
        frame.par_components = frame
            .par_components
            .checked_add(1)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    }

    match resolved.payload() {
        ResolvedProtobufPayload::Oneof(variant) => match variant.payload {
            RhoMatchPayloadKind::Message(type_id) => {
                let child = child_program(frame, &field, type_id)?;
                let body = field.into_length_delimited().map_err(|source| {
                    CanonicalProtobufAdmissionError::Framing {
                        type_name: frame.program.type_name,
                        region_offset: frame.region_offset,
                        source,
                    }
                })?;
                frame.waiting = Some(MessageWait::Child {
                    slot,
                    variant: Some(variant.name),
                    body: body.clone(),
                    field_offset,
                });
                Ok(FrameAction::Push(Frame::Message(MessageFrame::new(
                    body, child,
                )?)))
            }
            _ => {
                validate_scalar(frame, &field, variant.protobuf_kind, ScalarPresence::Oneof)?;
                record_scalar(
                    frame,
                    &field,
                    slot,
                    Some(variant.name),
                    variant.protobuf_kind,
                )?;
                Ok(FrameAction::Continue)
            }
        },
        ResolvedProtobufPayload::Declared(kind) => match schema_field.kind {
            RhoMatchFieldKind::Bool
            | RhoMatchFieldKind::I32
            | RhoMatchFieldKind::U32
            | RhoMatchFieldKind::I64
            | RhoMatchFieldKind::U64
            | RhoMatchFieldKind::Bytes
            | RhoMatchFieldKind::String => {
                validate_scalar(frame, &field, kind, ScalarPresence::Plain)?;
                record_scalar(frame, &field, slot, None, kind)?;
                Ok(FrameAction::Continue)
            }
            RhoMatchFieldKind::StringSequence | RhoMatchFieldKind::BytesSequence => {
                validate_scalar(frame, &field, kind, ScalarPresence::Repeated)?;
                record_scalar(frame, &field, slot, None, kind)?;
                Ok(FrameAction::Continue)
            }
            RhoMatchFieldKind::OptionalMessage(type_id)
            | RhoMatchFieldKind::MessageSequence(type_id) => {
                let child = child_program(frame, &field, type_id)?;
                let body = field.into_length_delimited().map_err(|source| {
                    CanonicalProtobufAdmissionError::Framing {
                        type_name: frame.program.type_name,
                        region_offset: frame.region_offset,
                        source,
                    }
                })?;
                frame.waiting = Some(MessageWait::Child {
                    slot,
                    variant: None,
                    body: body.clone(),
                    field_offset,
                });
                Ok(FrameAction::Push(Frame::Message(MessageFrame::new(
                    body, child,
                )?)))
            }
            RhoMatchFieldKind::StringMessageMap(type_id) => {
                let field_number = field.number;
                let body = field.into_length_delimited().map_err(|source| {
                    CanonicalProtobufAdmissionError::Framing {
                        type_name: frame.program.type_name,
                        region_offset: frame.region_offset,
                        source,
                    }
                })?;
                frame.waiting = Some(MessageWait::Map { slot, field_offset });
                Ok(FrameAction::Push(Frame::Map(MapFrame::new(
                    frame.program,
                    field_number,
                    type_id,
                    body,
                ))))
            }
            RhoMatchFieldKind::PathMap => {
                let field_number = field.number;
                let body = field.into_length_delimited().map_err(|source| {
                    CanonicalProtobufAdmissionError::Framing {
                        type_name: frame.program.type_name,
                        region_offset: frame.region_offset,
                        source,
                    }
                })?;
                let region_offset = body.absolute_range().start;
                let cursor = OwnedCanonicalEpm1TopologyCursor::new(body.clone().into_bytes())
                    .map_err(|source| CanonicalProtobufAdmissionError::EPathMap {
                        type_name: frame.program.type_name,
                        field: field_number,
                        region_offset,
                        source,
                    })?;
                stats.epathmaps = stats
                    .epathmaps
                    .checked_add(1)
                    .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
                frame.waiting = Some(MessageWait::EPathMap { slot, field_offset });
                Ok(FrameAction::Push(Frame::EPathMap(EPathMapFrame {
                    owner: frame.program,
                    field: field_number,
                    region_offset,
                    snapshot: body,
                    cursor,
                    path: Vec::new(),
                    entries_canonical_path_stable: true,
                    waiting_value: false,
                })))
            }
            RhoMatchFieldKind::Oneof(_) => {
                Err(frame.field_error(&field, "unresolved oneof marker reached admission"))
            }
            RhoMatchFieldKind::ParBag => Err(frame.field_error(
                &field,
                "native ParBag field appeared in a protobuf wire program",
            )),
        },
    }
}

fn map_step(
    frame: &mut MapFrame,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<FrameAction, CanonicalProtobufAdmissionError> {
    if frame.waiting_value {
        return Err(frame.schema_error("map entry resumed before its value returned"));
    }
    let field =
        frame
            .cursor
            .next_field()
            .map_err(|source| CanonicalProtobufAdmissionError::Framing {
                type_name: frame.owner.type_name,
                region_offset: frame.region_offset,
                source,
            })?;
    let Some(field) = field else {
        return Ok(FrameAction::Complete(FrameSummary::Map(frame.finish()?)));
    };
    stats.fields = stats
        .fields
        .checked_add(1)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let field_offset = frame
        .region_offset
        .checked_add(field.encoded_range.start)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    match field.number {
        1 if frame.key.is_none() => {
            let value = field.into_length_delimited().map_err(|source| {
                CanonicalProtobufAdmissionError::Framing {
                    type_name: frame.owner.type_name,
                    region_offset: frame.region_offset,
                    source,
                }
            })?;
            std::str::from_utf8(value.as_bytes()).map_err(|_| {
                frame.schema_error_at(1, field_offset, "protobuf map key is not UTF-8")
            })?;
            if value.is_empty() {
                return Err(frame.schema_error_at(
                    1,
                    field_offset,
                    "explicit protobuf map key equals the omitted default",
                ));
            }
            frame.key = Some(value);
            frame.key_offset = Some(field_offset);
            Ok(FrameAction::Continue)
        }
        2 if !frame.saw_value => {
            let value = field.into_length_delimited().map_err(|source| {
                CanonicalProtobufAdmissionError::Framing {
                    type_name: frame.owner.type_name,
                    region_offset: frame.region_offset,
                    source,
                }
            })?;
            let program = wire_program_by_type_id(frame.value_type_id)
                .map_err(|error| frame.schema_error_at(2, field_offset, error.detail()))?;
            frame.saw_value = true;
            frame.value_offset = Some(field_offset);
            frame.waiting_value = true;
            Ok(FrameAction::Push(Frame::Message(MessageFrame::new(
                value, program,
            )?)))
        }
        1 | 2 => Err(frame.field_error(&field, "canonical protobuf map-entry field is duplicated")),
        _ => Err(frame.field_error(
            &field,
            "field is outside the canonical protobuf map-entry schema",
        )),
    }
}

fn epathmap_step(
    frame: &mut EPathMapFrame,
) -> Result<FrameAction, CanonicalProtobufAdmissionError> {
    if frame.waiting_value {
        return Err(CanonicalProtobufAdmissionError::Schema {
            type_name: frame.owner.type_name,
            field: frame.field,
            region_offset: frame.region_offset,
            detail: "EPM1 cursor resumed before its value returned",
        });
    }
    loop {
        let Some(event) = frame.cursor.next_event() else {
            return Ok(FrameAction::Complete(FrameSummary::EPathMap(
                EPathMapSummary {
                    snapshot: frame.snapshot.clone(),
                    entries_canonical_path_stable: frame.entries_canonical_path_stable,
                },
            )));
        };
        match event {
            OwnedCanonicalEpm1TopologyEvent::Edge { byte, .. } => frame.path.push(byte),
            OwnedCanonicalEpm1TopologyEvent::Leave { .. } => {
                let _ = frame.path.pop();
            }
            OwnedCanonicalEpm1TopologyEvent::Value {
                has_value: false, ..
            } => {}
            OwnedCanonicalEpm1TopologyEvent::Value {
                has_value: true,
                value_body_range,
                source_offset,
                ..
            } => {
                let metadata = crate::rust::canonical_path::validate_canonical_path(&frame.path)
                    .map_err(|source| CanonicalProtobufAdmissionError::CanonicalPath {
                        type_name: frame.owner.type_name,
                        field: frame.field,
                        region_offset: frame.region_offset,
                        source_offset,
                        detail: source.to_string(),
                    })?;
                frame.entries_canonical_path_stable &= metadata.stable;

                let Some(range) = value_body_range else {
                    continue;
                };
                let value_offset = frame
                    .region_offset
                    .checked_add(range.start)
                    .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
                let value = frame
                    .snapshot
                    .subregion(super::protobuf_cursor::CanonicalByteRange {
                        start: range.start,
                        end: range.end,
                    })
                    .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
                frame.waiting_value = true;
                return Ok(FrameAction::Push(Frame::Message(MessageFrame::new_at(
                    value,
                    &PAR_PROTOBUF_REGION_PROGRAM,
                    value_offset,
                )?)));
            }
        }
    }
}

/// Validate one exact canonical protobuf encoder image without constructing a
/// rhoapi value. The returned statistics are operational evidence only; the
/// admission proof is represented by successful completion.
fn first_score_difference(
    scores: &CanonicalScoreArena,
    score: CanonicalScoreHandle,
    claimed: &[u8],
) -> Option<usize> {
    let mut expected = scores.cursor(score);
    let mut at = 0usize;
    loop {
        match (expected.next(), claimed.get(at).copied()) {
            (Some(left), Some(right)) if left == right => at += 1,
            (None, None) => return None,
            _ => return Some(at),
        }
    }
}

fn validate_canonical_protobuf_inner(
    input: ProtobufRegion,
    root: &'static RhoMatchProgram,
    claimed_component: Option<(ParComponentTag, &[u8])>,
) -> Result<CanonicalProtobufAdmission, CanonicalProtobufAdmissionError> {
    let mut scores = CanonicalScoreArena::for_region(&input)?;
    let mut stats = CanonicalProtobufAdmissionStats {
        messages: 1,
        max_frames: 1,
        ..CanonicalProtobufAdmissionStats::default()
    };
    let mut frames = vec![Frame::Message(MessageFrame::new(input, root)?)];
    loop {
        let action = match frames
            .last_mut()
            .expect("canonical protobuf admission retains its root frame")
        {
            Frame::Message(frame) => message_step(frame, &mut stats, &mut scores)?,
            Frame::Map(frame) => map_step(frame, &mut stats)?,
            Frame::EPathMap(frame) => epathmap_step(frame)?,
        };
        match action {
            FrameAction::Continue => {}
            FrameAction::Push(frame) => {
                match frame {
                    Frame::Message(_) => {
                        stats.messages = stats
                            .messages
                            .checked_add(1)
                            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?
                    }
                    Frame::Map(_) => {
                        stats.map_entries = stats
                            .map_entries
                            .checked_add(1)
                            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?
                    }
                    Frame::EPathMap(_) => {}
                }
                frames.push(frame);
                stats.max_frames = stats.max_frames.max(frames.len());
            }
            FrameAction::Complete(summary) => {
                frames.pop();
                if let Some(parent) = frames.last_mut() {
                    match parent {
                        Frame::Message(frame) => frame.accept(summary, &scores)?,
                        Frame::Map(frame) => frame.accept(summary)?,
                        Frame::EPathMap(frame) => frame.accept(summary)?,
                    }
                } else {
                    let FrameSummary::Message(summary) = summary else {
                        return Err(CanonicalProtobufAdmissionError::Schema {
                            type_name: root.type_name,
                            field: 0,
                            region_offset: 0,
                            detail: "canonical protobuf root returned the wrong summary kind",
                        });
                    };
                    let semantic_default_par = match &summary.semantic {
                        SemanticSummary::Par(par) => Some(par.semantic_default),
                        _ => None,
                    };
                    let canonical_path_stable_par = match &summary.semantic {
                        SemanticSummary::Par(par) => Some(par.canonical_path_stable),
                        _ => None,
                    };
                    let component = match &summary.semantic {
                        SemanticSummary::Component(component) => {
                            Some(CanonicalComponentAdmission {
                                family: component.family,
                                canonical_path_stable: component.canonical_path_stable,
                                complete: component.complete,
                            })
                        }
                        _ => None,
                    };
                    if let Some((family, claimed_score)) = claimed_component {
                        let SemanticSummary::Component(root_component) = &summary.semantic else {
                            return Err(CanonicalProtobufAdmissionError::Schema {
                                type_name: root.type_name,
                                field: 0,
                                region_offset: 0,
                                detail:
                                    "claimed component score was supplied for a non-component root",
                            });
                        };
                        if root_component.family != family {
                            return Err(CanonicalProtobufAdmissionError::Schema {
                                type_name: root.type_name,
                                field: u32::from(family.byte()),
                                region_offset: 0,
                                detail: "claimed component family disagrees with the schema root",
                            });
                        }
                        if let Some(first_difference) =
                            first_score_difference(&scores, root_component.score, claimed_score)
                        {
                            return Err(CanonicalProtobufAdmissionError::ScoreMismatch {
                                type_name: root.type_name,
                                region_offset: 0,
                                first_difference,
                            });
                        }
                    }
                    return Ok(CanonicalProtobufAdmission {
                        stats,
                        semantic_default_par,
                        canonical_path_stable_par,
                        component,
                    });
                }
            }
        }
    }
}

pub(crate) fn validate_canonical_protobuf(
    input: ProtobufRegion,
    root: &'static RhoMatchProgram,
) -> Result<CanonicalProtobufAdmission, CanonicalProtobufAdmissionError> {
    validate_canonical_protobuf_inner(input, root, None)
}

pub(crate) fn validate_canonical_component_protobuf(
    input: ProtobufRegion,
    root: &'static RhoMatchProgram,
    family: ParComponentTag,
    claimed_score: &[u8],
) -> Result<CanonicalComponentAdmission, CanonicalProtobufAdmissionError> {
    validate_canonical_protobuf_inner(input, root, Some((family, claimed_score)))?
        .component()
        .ok_or(CanonicalProtobufAdmissionError::Schema {
            type_name: root.type_name,
            field: u32::from(family.byte()),
            region_offset: 0,
            detail: "component admission root did not produce component metadata",
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::rhoapi::connective::ConnectiveInstance;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::g_unforgeable::UnfInstance;
    use crate::rhoapi::{
        Bundle, Connective, ConnectiveBody, EList, Expr, GPrivate, GUnforgeable, If, Match,
        MatchCase, New, Par, Receive, ReceiveBind, Send,
    };
    use crate::rust::par_bag::{with_component_key, ParBagComponent, PAR_BAG_KEY_DOMAIN};
    use crate::rust::rhoapi_ext::EPathMap;
    use crate::rust::rholang::match_schema_tables::{
        BUNDLE_RHO_MATCH_PROGRAM, CONNECTIVE_RHO_MATCH_PROGRAM, EXPR_RHO_MATCH_PROGRAM,
        GUNFORGEABLE_RHO_MATCH_PROGRAM, IF_RHO_MATCH_PROGRAM, MATCH_RHO_MATCH_PROGRAM,
        NEW_RHO_MATCH_PROGRAM, PAR_RHO_MATCH_PROGRAM, RECEIVE_RHO_MATCH_PROGRAM,
        SEND_RHO_MATCH_PROGRAM,
    };
    use crate::rust::rholang::protobuf_encoder;
    use crate::rust::rholang::protobuf_schema::ProtobufField;

    fn gint(value: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }])
    }

    fn validate(bytes: Vec<u8>, program: &'static RhoMatchProgram) -> CanonicalProtobufAdmission {
        validate_canonical_protobuf(ProtobufRegion::from(bytes), program)
            .unwrap_or_else(|error| panic!("canonical encoder image was rejected: {error}"))
    }

    fn rejects(bytes: Vec<u8>, program: &'static RhoMatchProgram) {
        assert!(
            validate_canonical_protobuf(ProtobufRegion::from(bytes), program).is_err(),
            "noncanonical protobuf image was admitted"
        );
    }

    fn assert_component_score_state<T: ParBagComponent>(
        value: &T,
        program: &'static RhoMatchProgram,
        expected_complete: bool,
    ) {
        with_component_key(value, |key| {
            let trailer = key.len() - std::mem::size_of::<u64>();
            let score_len = usize::try_from(u64::from_be_bytes(
                key[trailer..]
                    .try_into()
                    .expect("an RPB3 key ends in one score-length word"),
            ))
            .expect("the test score length fits usize");
            let score_start = PAR_BAG_KEY_DOMAIN.len() + 1;
            let score_end = score_start + score_len;
            let admission = validate_canonical_component_protobuf(
                ProtobufRegion::copy_from_slice(&key[score_end..trailer]),
                program,
                T::TAG,
                &key[score_start..score_end],
            )
            .unwrap_or_else(|error| panic!("canonical RPB3 component was rejected: {error}"));
            assert_eq!(admission.family, T::TAG);
            assert_eq!(admission.complete, expected_complete);
            assert_eq!(
                admission.canonical_path_stable,
                T::canonical_path_stable(value)
            );

            let mut wrong = key[score_start..score_end].to_vec();
            let middle = wrong.len() / 2;
            wrong[middle] ^= 1;
            assert!(matches!(
                validate_canonical_component_protobuf(
                    ProtobufRegion::copy_from_slice(&key[score_end..trailer]),
                    program,
                    T::TAG,
                    &wrong,
                ),
                Err(CanonicalProtobufAdmissionError::ScoreMismatch { .. })
            ));
        });
    }

    fn assert_component_score<T: ParBagComponent>(value: &T, program: &'static RhoMatchProgram) {
        assert_component_score_state(value, program, true);
    }

    #[test]
    fn every_component_family_matches_the_authoritative_rpb3_score() {
        let empty = Par::default();
        assert_component_score(
            &Send {
                chan: Some(empty.clone()),
                data: vec![gint(1)],
                persistent: true,
                connective_used: true,
                ..Send::default()
            },
            &SEND_RHO_MATCH_PROGRAM,
        );
        assert_component_score(
            &Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![gint(2)],
                    source: Some(empty.clone()),
                    ..ReceiveBind::default()
                }],
                body: Some(empty.clone()),
                condition: Some(gint(3)),
                persistent: true,
                peek: true,
                bind_count: 1,
                connective_used: true,
                ..Receive::default()
            },
            &RECEIVE_RHO_MATCH_PROGRAM,
        );
        assert_component_score(
            &New {
                bind_count: 2,
                p: Some(empty.clone()),
                uri: vec!["rho:a".to_string(), "rho:z".to_string()],
                injections: BTreeMap::from([("x".to_string(), gint(4))]),
                ..New::default()
            },
            &NEW_RHO_MATCH_PROGRAM,
        );
        assert_component_score(
            &Expr {
                expr_instance: Some(ExprInstance::GInt(-7)),
            },
            &EXPR_RHO_MATCH_PROGRAM,
        );
        assert_component_score(
            &Match {
                target: Some(empty.clone()),
                cases: vec![MatchCase {
                    pattern: Some(gint(5)),
                    source: Some(gint(6)),
                    guard: Some(gint(7)),
                    ..MatchCase::default()
                }],
                connective_used: true,
                ..Match::default()
            },
            &MATCH_RHO_MATCH_PROGRAM,
        );
        assert_component_score(
            &Bundle {
                body: Some(empty.clone()),
                read_flag: true,
                write_flag: false,
            },
            &BUNDLE_RHO_MATCH_PROGRAM,
        );
        assert_component_score(
            &Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![gint(8), gint(9)],
                })),
            },
            &CONNECTIVE_RHO_MATCH_PROGRAM,
        );
        assert_component_score(
            &GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![1, 2, 3] })),
            },
            &GUNFORGEABLE_RHO_MATCH_PROGRAM,
        );
        assert_component_score(
            &If {
                condition: Some(gint(10)),
                if_true: Some(empty.clone()),
                if_false: Some(gint(11)),
                connective_used: true,
                ..If::default()
            },
            &IF_RHO_MATCH_PROGRAM,
        );
    }

    #[test]
    fn every_incomplete_component_shape_matches_the_nonsemantic_rpb3_score() {
        assert_component_score_state(&Send::default(), &SEND_RHO_MATCH_PROGRAM, false);
        assert_component_score_state(&Receive::default(), &RECEIVE_RHO_MATCH_PROGRAM, false);
        assert_component_score_state(&New::default(), &NEW_RHO_MATCH_PROGRAM, false);
        assert_component_score_state(&Match::default(), &MATCH_RHO_MATCH_PROGRAM, false);
        assert_component_score_state(&Bundle::default(), &BUNDLE_RHO_MATCH_PROGRAM, false);
        assert_component_score_state(&If::default(), &IF_RHO_MATCH_PROGRAM, false);

        let child = Some(gint(1));
        let incomplete_exprs = vec![
            ExprInstance::EVarBody(crate::rhoapi::EVar { v: None }),
            ExprInstance::ENegBody(crate::rhoapi::ENeg { p: None }),
            ExprInstance::ENotBody(crate::rhoapi::ENot { p: None }),
            ExprInstance::EMultBody(crate::rhoapi::EMult {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EDivBody(crate::rhoapi::EDiv {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EModBody(crate::rhoapi::EMod {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EPlusBody(crate::rhoapi::EPlus {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EMinusBody(crate::rhoapi::EMinus {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::ELtBody(crate::rhoapi::ELt {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::ELteBody(crate::rhoapi::ELte {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EGtBody(crate::rhoapi::EGt {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EGteBody(crate::rhoapi::EGte {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EEqBody(crate::rhoapi::EEq {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::ENeqBody(crate::rhoapi::ENeq {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EAndBody(crate::rhoapi::EAnd {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EOrBody(crate::rhoapi::EOr {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EMatchesBody(crate::rhoapi::EMatches {
                target: child.clone(),
                pattern: None,
            }),
            ExprInstance::EPercentPercentBody(crate::rhoapi::EPercentPercent {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EPlusPlusBody(crate::rhoapi::EPlusPlus {
                p1: child.clone(),
                p2: None,
            }),
            ExprInstance::EMinusMinusBody(crate::rhoapi::EMinusMinus {
                p1: child,
                p2: None,
            }),
            ExprInstance::EMapBody(crate::rhoapi::EMap {
                kvs: vec![crate::rhoapi::KeyValuePair {
                    key: Some(gint(2)),
                    value: None,
                }],
                ..crate::rhoapi::EMap::default()
            }),
            ExprInstance::EMethodBody(crate::rhoapi::EMethod {
                target: None,
                ..crate::rhoapi::EMethod::default()
            }),
            ExprInstance::EZipperBody(crate::rhoapi::EZipper::default()),
        ];
        for expr_instance in incomplete_exprs {
            assert_component_score_state(
                &Expr {
                    expr_instance: Some(expr_instance),
                },
                &EXPR_RHO_MATCH_PROGRAM,
                false,
            );
        }
    }

    #[test]
    fn complete_lane_rejects_sorter_normalization_aliases() {
        rejects(
            protobuf_encoder::encode_to_vec(&New {
                p: Some(Par::default()),
                uri: vec!["z".to_string(), "a".to_string()],
                ..New::default()
            }),
            &NEW_RHO_MATCH_PROGRAM,
        );
        rejects(
            protobuf_encoder::encode_to_vec(&Receive {
                body: Some(Par::default()),
                condition: Some(Par::default()),
                ..Receive::default()
            }),
            &RECEIVE_RHO_MATCH_PROGRAM,
        );
        rejects(
            protobuf_encoder::encode_to_vec(&Match {
                target: Some(Par::default()),
                cases: vec![MatchCase {
                    pattern: Some(Par::default()),
                    source: Some(Par::default()),
                    guard: Some(Par::default()),
                    ..MatchCase::default()
                }],
                ..Match::default()
            }),
            &MATCH_RHO_MATCH_PROGRAM,
        );
    }

    #[test]
    fn canonical_encoder_images_validate_without_constructing_an_ast() {
        let send = Send {
            chan: Some(gint(1)),
            data: vec![gint(2), gint(3)],
            persistent: true,
            locally_free: vec![1, 5, 9],
            connective_used: true,
        };
        let send_bytes = protobuf_encoder::encode_to_vec(&send);
        let admission = validate(send_bytes, &SEND_RHO_MATCH_PROGRAM);
        // Send root + three Par children + their three scalar Expr children.
        assert_eq!(admission.stats().messages, 7);
        assert_eq!(admission.semantic_default_par(), None);

        let par = Par::default()
            .with_sends(vec![send])
            .with_connective_used(true);
        let par_bytes = protobuf_encoder::encode_to_vec(&par);
        let admission = validate(par_bytes, &PAR_PROTOBUF_REGION_PROGRAM);
        assert_eq!(admission.semantic_default_par(), Some(false));
        assert_eq!(admission.canonical_path_stable_par(), Some(false));
    }

    #[test]
    fn raw_par_default_equality_ignores_only_root_locally_free() {
        let default = validate(Vec::new(), &PAR_PROTOBUF_REGION_PROGRAM);
        assert_eq!(default.semantic_default_par(), Some(true));
        assert_eq!(default.canonical_path_stable_par(), Some(false));

        let locally_free_only = Par::default().with_locally_free(vec![7]);
        let admission = validate(
            protobuf_encoder::encode_to_vec(&locally_free_only),
            &PAR_PROTOBUF_REGION_PROGRAM,
        );
        assert_eq!(admission.semantic_default_par(), Some(true));
        assert_eq!(admission.canonical_path_stable_par(), Some(false));

        let connective = Par::default().with_connective_used(true);
        let admission = validate(
            protobuf_encoder::encode_to_vec(&connective),
            &PAR_PROTOBUF_REGION_PROGRAM,
        );
        assert_eq!(admission.semantic_default_par(), Some(false));
        assert_eq!(admission.canonical_path_stable_par(), Some(false));

        let stable = validate(
            protobuf_encoder::encode_to_vec(&gint(7)),
            &PAR_PROTOBUF_REGION_PROGRAM,
        );
        assert_eq!(stable.semantic_default_par(), Some(false));
        assert_eq!(stable.canonical_path_stable_par(), Some(true));
    }

    #[test]
    fn admission_rejects_native_or_mismatched_schema_tables_without_panicking() {
        rejects(Vec::new(), &PAR_RHO_MATCH_PROGRAM);

        static STRAY_PROTOBUF_FIELD: [ProtobufField; 1] = [ProtobufField {
            tag: 1,
            kind: ProtobufKind::Bool,
            name: "stray",
        }];
        static MISMATCHED_PROGRAM: RhoMatchProgram = RhoMatchProgram {
            type_id: u16::MAX,
            type_name: "MismatchedProgram",
            fields: &[],
            protobuf_fields: &STRAY_PROTOBUF_FIELD,
        };
        rejects(Vec::new(), &MISMATCHED_PROGRAM);
    }

    #[test]
    fn map_entries_follow_btree_order_and_prost_default_omission() {
        let mut injections = BTreeMap::new();
        injections.insert(String::new(), Par::default());
        injections.insert("alpha".to_string(), gint(1));
        injections.insert("omega".to_string(), gint(2));
        let value = New {
            bind_count: -7,
            p: Some(gint(9)),
            uri: vec![String::new(), "rho:uri".to_string()],
            injections,
            locally_free: vec![3],
        };
        let admission = validate(
            protobuf_encoder::encode_to_vec(&value),
            &NEW_RHO_MATCH_PROGRAM,
        );
        assert_eq!(admission.stats().map_entries, 3);

        // An empty key/default value is represented by an empty entry body.
        validate(vec![0x22, 0x00], &NEW_RHO_MATCH_PROGRAM);
        // Presence of either omitted default is noncanonical.
        rejects(vec![0x22, 0x02, 0x0a, 0x00], &NEW_RHO_MATCH_PROGRAM);
        rejects(vec![0x22, 0x02, 0x12, 0x00], &NEW_RHO_MATCH_PROGRAM);
        // `Par::PartialEq` ignores root locally_free, so this value is also the
        // map default even though its protobuf body is nonempty.
        rejects(
            vec![0x22, 0x05, 0x12, 0x03, 0x4a, 0x01, 0x01],
            &NEW_RHO_MATCH_PROGRAM,
        );

        // Two individually canonical entries become noncanonical when their
        // outer repeated-field order disagrees with BTreeMap key order.
        let alpha = [0x22, 0x07, 0x0a, 0x05, b'a', b'l', b'p', b'h', b'a'];
        let omega = [0x22, 0x07, 0x0a, 0x05, b'o', b'm', b'e', b'g', b'a'];
        let mut reversed = omega.to_vec();
        reversed.extend_from_slice(&alpha);
        rejects(reversed, &NEW_RHO_MATCH_PROGRAM);
    }

    #[test]
    fn scalar_aliases_defaults_unknowns_and_duplicate_oneofs_are_rejected() {
        // Send.persistent = false is omitted by Prost.
        rejects(vec![0x18, 0x00], &SEND_RHO_MATCH_PROGRAM);
        // Raw int32 2^32-1 decodes to -1 but is not its ten-byte canonical
        // sign-extended representation.
        rejects(
            vec![0x20, 0xff, 0xff, 0xff, 0xff, 0x0f],
            &crate::rust::rholang::match_schema_tables::RECEIVEBIND_RHO_MATCH_PROGRAM,
        );
        // Expr GString accepts default presence because it is a oneof, but it
        // still requires UTF-8 and a unique oneof slot.
        validate(vec![0x1a, 0x00], &EXPR_RHO_MATCH_PROGRAM);
        rejects(vec![0x1a, 0x01, 0xff], &EXPR_RHO_MATCH_PROGRAM);
        rejects(vec![0x10, 0x01, 0x10, 0x02], &EXPR_RHO_MATCH_PROGRAM);
        // Unknown field and nonminimal varint framing never enter the schema.
        rejects(vec![0xf8, 0x07, 0x01], &SEND_RHO_MATCH_PROGRAM);
        rejects(vec![0x18, 0x81, 0x00], &SEND_RHO_MATCH_PROGRAM);
    }

    #[test]
    fn epathmap_snapshot_and_every_nested_map_value_are_validated() {
        let map = EPathMap::new_map(
            [(gint(1), gint(11)), (gint(2), gint(22))],
            Vec::new(),
            false,
            None,
        );
        let expr = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        };
        let admission = validate(
            protobuf_encoder::encode_to_vec(&expr),
            &EXPR_RHO_MATCH_PROGRAM,
        );
        assert_eq!(admission.stats().epathmaps, 1);

        // The hand-written canonical EPathMap writer always emits tag 9,
        // including neutral empty mode.
        rejects(vec![0x82, 0x02, 0x00], &EXPR_RHO_MATCH_PROGRAM);
    }

    #[test]
    fn depth_4096_validation_uses_the_heap_worklist_on_a_small_native_stack() {
        let mut deep = gint(0);
        for _ in 0..4_096 {
            deep = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![deep],
                    ..EList::default()
                })),
            }]);
        }
        let bytes = protobuf_encoder::encode_to_vec(&deep);
        std::thread::Builder::new()
            .name("canonical-protobuf-admission-depth".to_string())
            .stack_size(256 * 1024)
            .spawn(move || {
                let admission = validate(bytes, &PAR_PROTOBUF_REGION_PROGRAM);
                assert!(admission.stats().max_frames >= 4_096);
            })
            .expect("spawn small-stack canonical protobuf validator")
            .join()
            .expect("canonical protobuf validator completes");
    }
}
