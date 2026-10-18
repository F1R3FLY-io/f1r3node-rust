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
use rspace_plus_plus::rspace::pathmap_codec::CanonicalCompactTrieLineDirection;

use super::canonical_bytes_arena::CanonicalByteProvenance;
#[cfg(test)]
use super::canonical_bytes_arena::CanonicalBytesArena;
use super::canonical_path_admission::{
    CanonicalPathCompletedObligation, CanonicalPathObligation, CanonicalPathObligationId,
    CanonicalPathObligationKind, CanonicalPathObligationResolution, CanonicalPathSource,
    CanonicalPathStreamReducer, CanonicalPathSummary, CanonicalPathTopologyAdmission,
    CanonicalPathTransition, LocatedPathByte, NestedEpmMode,
};
use super::canonical_score_bytes::{
    CanonicalScoreArena, CanonicalScoreArenaError, CanonicalScoreHandle,
};
use super::canonical_semantics::{
    self as semantics, CanonicalScoreAlgebra, ParComponentTag, PreScoredGroundScore,
    PreScoredUnforgeableScore, Score,
};
use super::canonical_source_tree::SourceTreeWindow;
#[cfg(test)]
use super::indexed_protobuf::OwnedIndexedByteRegion;
use super::indexed_protobuf::{
    IndexedBytePosition, IndexedByteRegion, IndexedCanonicalField, IndexedCanonicalFieldCursor,
    IndexedCanonicalFieldValue, IndexedLocatedByteCursor, LocatedCanonicalProtobufError,
};
use super::match_schema::{
    resolve_protobuf_field, wire_program_by_type_id, ResolvedProtobufField,
    ResolvedProtobufPayload, RhoMatchFieldKind, RhoMatchPayloadKind, RhoMatchProgram,
};
use super::match_schema_tables::*;
#[cfg(test)]
use super::protobuf_cursor::ProtobufRegion;
use super::protobuf_cursor::{CanonicalByteRange, CanonicalProtobufError, CanonicalWireType};
use super::protobuf_schema::{ProtobufKind, ProtobufWireType};
use crate::rust::epathmap_trie_codec::{
    EPathMapMode, IndexedCanonicalEpm1TopologyCursor, IndexedCanonicalEpm1TopologyEvent,
    TrieCodecError,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CanonicalProtobufAdmissionStats {
    pub(crate) input_bytes: usize,
    pub(crate) protobuf_bytes_read: usize,
    pub(crate) protobuf_bytes_traversed: usize,
    pub(crate) protobuf_bytes_fast_forwarded: usize,
    pub(crate) fields: usize,
    pub(crate) messages: usize,
    pub(crate) map_entries: usize,
    pub(crate) epathmaps: usize,
    pub(crate) epathmap_events: usize,
    pub(crate) canonical_path_obligations: usize,
    pub(crate) canonical_path_payload_bytes: usize,
    pub(crate) canonical_path_bytes_read: usize,
    pub(crate) canonical_path_bytes_traversed: usize,
    pub(crate) canonical_path_resolutions: usize,
    pub(crate) max_frames: usize,
    pub(crate) frame_variant_bytes: usize,
    pub(crate) max_frame_storage_bytes: usize,
    pub(crate) max_frame_capacity_bytes: usize,
    pub(crate) reconstructed_key_bytes: usize,
    pub(crate) legacy_endpoint_replays: usize,
    pub(crate) typed_decodes: usize,
    pub(crate) byte_native_validations: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), derive(Copy))]
pub(crate) struct CanonicalProtobufAdmission {
    stats: CanonicalProtobufAdmissionStats,
    semantic_default_par: Option<bool>,
    canonical_path_stable_par: Option<bool>,
    component: Option<CanonicalComponentAdmission>,
    /// Exact root score emitted by the byte-native semantic reducer. This is
    /// retained only by tests so the executable differential can observe the
    /// reducer independently of the claimed-score comparison wrapper.
    #[cfg(test)]
    score_bytes: Option<Vec<u8>>,
}

impl CanonicalProtobufAdmission {
    #[cfg(test)]
    pub(crate) const fn stats(&self) -> CanonicalProtobufAdmissionStats { self.stats }

    /// `Some(true)` means the admitted root is a raw protobuf `Par` equal to
    /// `Par::default()`. Root `locally_free` is intentionally ignored, exactly
    /// as `Par::PartialEq` ignores it and Prost's map-value default test does.
    #[cfg(test)]
    pub(crate) const fn semantic_default_par(&self) -> Option<bool> { self.semantic_default_par }

    /// `Some(stable)` is the exact canonical-path stability verdict for an
    /// admitted raw `Par`.  Nested path admission uses this to resolve an
    /// escape payload without constructing a rhoapi value.
    #[cfg(test)]
    pub(crate) const fn canonical_path_stable_par(&self) -> Option<bool> {
        self.canonical_path_stable_par
    }

    #[cfg(test)]
    pub(crate) fn score_bytes(&self) -> Option<&[u8]> { self.score_bytes.as_deref() }

    pub(crate) const fn component(&self) -> Option<CanonicalComponentAdmission> { self.component }
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
        region_start: CanonicalByteProvenance,
        physical: IndexedBytePosition,
        source: CanonicalProtobufError,
    },
    Schema {
        type_name: &'static str,
        field: u32,
        position: CanonicalByteProvenance,
        detail: &'static str,
    },
    EPathMap {
        type_name: &'static str,
        field: u32,
        position: CanonicalByteProvenance,
        source: TrieCodecError,
    },
    CanonicalPath {
        type_name: &'static str,
        field: u32,
        position: CanonicalPathSource,
        detail: String,
    },
    ScoreMismatch {
        type_name: &'static str,
        region_start: CanonicalByteProvenance,
        first_difference: usize,
    },
    CutoverInvariant(&'static str),
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
                region_start,
                physical,
                source,
            } => write!(
                formatter,
                "{type_name} protobuf region beginning at source {} byte {}, failure at source {} byte {}: {source}",
                region_start.source,
                region_start.offset,
                physical.physical.source,
                physical.physical.offset,
            ),
            Self::Schema {
                type_name,
                field,
                position,
                detail,
            } => write!(
                formatter,
                "{type_name} protobuf field {field} at source {} byte {}: {detail}",
                position.source, position.offset,
            ),
            Self::EPathMap {
                type_name,
                field,
                position,
                source,
            } => write!(
                formatter,
                "{type_name} protobuf field {field} EPM1 at source {} byte {}: {source}",
                position.source, position.offset,
            ),
            Self::CanonicalPath {
                type_name,
                field,
                position,
                detail,
            } => write!(
                formatter,
                "{type_name} protobuf field {field} canonical path at source {} byte {}: {detail}",
                position.source, position.offset,
            ),
            Self::ScoreMismatch {
                type_name,
                region_start,
                first_difference,
            } => write!(
                formatter,
                "{type_name} canonical score for source {} region beginning at byte {} differs at score byte {first_difference}",
                region_start.source, region_start.offset,
            ),
            Self::CutoverInvariant(detail) => {
                write!(
                    formatter,
                    "byte-native protobuf cutover invariant failed: {detail}"
                )
            }
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
            | Self::CutoverInvariant(_)
            | Self::SizeOverflow => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SemanticKind {
    /// A descriptor-known message with no RPB3 scoring role. Its fields are
    /// still validated recursively and canonically; it simply contributes no
    /// Rholang semantic-score summary to a parent.
    Opaque,
    RawPar,
    Component(ParComponentTag),
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
    } else if program_is!(COSTTOKENSTACK_RHO_MATCH_PROGRAM) {
        SemanticKind::Component(ParComponentTag::CostTokenStack)
    } else if program_is!(VAR_RHO_MATCH_PROGRAM) {
        SemanticKind::Var
    } else if program_is!(VAR_WILDCARDMSG_RHO_MATCH_PROGRAM)
        || program_is!(GSYSAUTHTOKEN_RHO_MATCH_PROGRAM)
    {
        SemanticKind::Unit
    } else if program_is!(RECEIVEBIND_RHO_MATCH_PROGRAM) {
        SemanticKind::ReceiveBind
    } else if program_is!(COSTSIGNATURE_RHO_MATCH_PROGRAM) {
        SemanticKind::CostSignature
    } else if program_is!(COSTSIGNATUREATOM_RHO_MATCH_PROGRAM) {
        SemanticKind::CostSignatureAtom
    } else if program_is!(COSTBOUNDSIGNATURE_RHO_MATCH_PROGRAM) {
        SemanticKind::CostBoundSignature
    } else if program_is!(COSTREGION_RHO_MATCH_PROGRAM) {
        SemanticKind::CostRegion
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
    } else if RHO_MATCH_REGISTRY
        .iter()
        .any(|(_, candidate)| std::ptr::eq(program, *candidate))
    {
        // The generated registry is the closed descriptor authority. Wrappers
        // such as PCost, ParWithRandom, and TaggedContinuation need strict wire
        // admission but are not members of the RPB3 scoring algebra.
        SemanticKind::Opaque
    } else {
        return None;
    };
    Some(kind)
}

type AdmissionRegion<'src> = IndexedByteRegion<'src>;

#[derive(Clone)]
enum SemanticScalar<'src> {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    String(AdmissionRegion<'src>),
    Bytes(AdmissionRegion<'src>),
}

struct SemanticOccurrence<'src> {
    variant: Option<&'static str>,
    body: Option<AdmissionRegion<'src>>,
    value: SemanticValue<'src>,
    field_position: CanonicalByteProvenance,
}

enum SemanticValue<'src> {
    Scalar(SemanticScalar<'src>),
    Message(Box<MessageSummary<'src>>),
    Map(Box<MapSummary<'src>>),
    EPathMap(Box<EPathMapSummary<'src>>),
}

#[derive(Clone)]
struct ComponentSummary {
    family: ParComponentTag,
    score: CanonicalScoreHandle,
    complete: bool,
    canonical_path_stable: bool,
}

#[derive(Clone)]
struct ParSummary<'src> {
    score: CanonicalScoreHandle,
    semantic_default: bool,
    canonical_path_stable: bool,
    identity_prefix: AdmissionRegion<'src>,
    identity_suffix: AdmissionRegion<'src>,
}

#[derive(Clone)]
struct ExprPayloadSummary<'src> {
    score: Option<CanonicalScoreHandle>,
    canonical_path_stable: bool,
    canonical_fixed_point: bool,
    pathmap_snapshot: Option<AdmissionRegion<'src>>,
}

#[derive(Clone)]
enum SemanticSummary<'src> {
    Opaque,
    Par(ParSummary<'src>),
    Component(ComponentSummary),
    Var(CanonicalScoreHandle),
    ReceiveBind {
        score: Option<CanonicalScoreHandle>,
        source_present: bool,
    },
    CostScore(CanonicalScoreHandle),
    MatchCase {
        score: Option<CanonicalScoreHandle>,
        required_present: bool,
        canonical_fixed_point: bool,
    },
    ExprPayload(ExprPayloadSummary<'src>),
    KeyValuePair {
        key: Option<ParSummary<'src>>,
        value: Option<ParSummary<'src>>,
    },
    BigRational {
        numerator: AdmissionRegion<'src>,
        denominator: AdmissionRegion<'src>,
    },
    FixedPoint {
        unscaled: AdmissionRegion<'src>,
        scale: u32,
    },
    ConnectiveBody(Vec<ParSummary<'src>>),
    VarRef {
        index: i32,
        depth: i32,
    },
    Bytes(AdmissionRegion<'src>),
    Unit,
}

#[derive(Clone)]
struct EPathMapSummary<'src> {
    mode: EPathMapMode,
    snapshot: AdmissionRegion<'src>,
    entries_canonical_path_stable: bool,
}

struct MessageFrame<'src> {
    program: &'static RhoMatchProgram,
    body: AdmissionRegion<'src>,
    region_start: CanonicalByteProvenance,
    cursor: IndexedCanonicalFieldCursor<'src>,
    last_slot: Option<usize>,
    seen: Vec<bool>,
    semantic_slots: Vec<Vec<SemanticOccurrence<'src>>>,
    waiting: Option<MessageWait<'src>>,
    par_components: u64,
    par_locally_free_range: Option<super::protobuf_cursor::CanonicalByteRange>,
}

enum MessageWait<'src> {
    Child {
        slot: usize,
        variant: Option<&'static str>,
        body: AdmissionRegion<'src>,
        field_position: CanonicalByteProvenance,
    },
    Map {
        slot: usize,
        field_position: CanonicalByteProvenance,
    },
    EPathMap {
        slot: usize,
        field_position: CanonicalByteProvenance,
    },
}

impl<'src> MessageFrame<'src> {
    fn from_message_region(
        region: AdmissionRegion<'src>,
        program: &'static RhoMatchProgram,
    ) -> Result<Self, CanonicalProtobufAdmissionError> {
        let region_start = region
            .position_at(0)
            .expect("indexed regions own their start")
            .physical;
        Self::from_message_region_at(region, program, region_start)
    }

    fn from_message_region_at(
        region: AdmissionRegion<'src>,
        program: &'static RhoMatchProgram,
        region_start: CanonicalByteProvenance,
    ) -> Result<Self, CanonicalProtobufAdmissionError> {
        if program.fields.len() != program.protobuf_fields.len() {
            return Err(CanonicalProtobufAdmissionError::Schema {
                type_name: program.type_name,
                field: 0,
                position: region_start,
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
                position: region_start,
                detail: "native ParBag program cannot validate protobuf wire bytes",
            });
        }
        let slots = program.fields.len();
        Ok(Self {
            program,
            body: region.clone(),
            region_start,
            cursor: IndexedCanonicalFieldCursor::in_prost_slot_order(region),
            last_slot: None,
            seen: vec![false; slots],
            semantic_slots: (0..slots).map(|_| Vec::new()).collect(),
            waiting: None,
            par_components: 0,
            par_locally_free_range: None,
        })
    }

    fn is_raw_par(&self) -> bool { std::ptr::eq(self.program, &PAR_PROTOBUF_REGION_PROGRAM) }

    fn schema_error(&self, field: u32, detail: &'static str) -> CanonicalProtobufAdmissionError {
        self.schema_error_at(field, self.region_start, detail)
    }

    fn schema_error_at(
        &self,
        field: u32,
        position: CanonicalByteProvenance,
        detail: &'static str,
    ) -> CanonicalProtobufAdmissionError {
        CanonicalProtobufAdmissionError::Schema {
            type_name: self.program.type_name,
            field,
            position,
            detail,
        }
    }

    fn field_error(
        &self,
        field: &IndexedCanonicalField<'src>,
        detail: &'static str,
    ) -> CanonicalProtobufAdmissionError {
        self.schema_error_at(field.number, field.field_start.physical, detail)
    }

    fn accept(
        &mut self,
        summary: FrameSummary<'src>,
        scores: &CanonicalScoreArena<'src>,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        match (self.waiting.take(), summary) {
            (
                Some(MessageWait::Child {
                    slot,
                    variant,
                    body,
                    field_position,
                }),
                FrameSummary::Message(summary),
            ) => {
                if self.is_raw_par() {
                    let SemanticSummary::Component(component) = &summary.semantic else {
                        return Err(self.schema_error_at(
                            self.program.fields[slot].tag,
                            body.position_at(0)
                                .expect("child bodies own their start")
                                .physical,
                            "raw Par component child returned the wrong semantic summary",
                        ));
                    };
                    if let Some(previous) = self.semantic_slots[slot].last() {
                        let (SemanticValue::Message(summary), Some(previous_body)) =
                            (&previous.value, &previous.body)
                        else {
                            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                                "retained raw Par history is not a component with a body",
                            ));
                        };
                        let SemanticSummary::Component(previous_component) = &summary.semantic
                        else {
                            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                                "retained raw Par history has no component score",
                            ));
                        };
                        let order = scores
                            .compare(previous_component.score, component.score)
                            .then_with(|| previous_body.compare(&body));
                        if order.is_gt() {
                            return Err(self.schema_error_at(
                                self.program.fields[slot].tag,
                                body.position_at(0)
                                    .expect("child bodies own their start")
                                    .physical,
                                "raw Par components are outside canonical score/body order",
                            ));
                        }
                    }
                }
                self.semantic_slots[slot].push(SemanticOccurrence {
                    variant,
                    body: Some(body),
                    value: SemanticValue::Message(Box::new(summary)),
                    field_position,
                });
                Ok(())
            }
            (
                Some(MessageWait::Map {
                    slot,
                    field_position,
                }),
                FrameSummary::Map(entry),
            ) => {
                if let Some(previous) = self.semantic_slots[slot].last() {
                    let SemanticValue::Map(previous) = &previous.value else {
                        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                            "retained map history is not a map entry",
                        ));
                    };
                    if !previous.key.compare(&entry.key).is_lt() {
                        return Err(self.schema_error_at(
                            self.program.fields[slot].tag,
                            entry.key_position,
                            "protobuf map keys are not strictly ascending",
                        ));
                    }
                }
                self.semantic_slots[slot].push(SemanticOccurrence {
                    variant: None,
                    body: None,
                    value: SemanticValue::Map(Box::new(entry)),
                    field_position,
                });
                Ok(())
            }
            (
                Some(MessageWait::EPathMap {
                    slot,
                    field_position,
                }),
                FrameSummary::EPathMap(summary),
            ) => {
                self.semantic_slots[slot].push(SemanticOccurrence {
                    variant: None,
                    body: Some(summary.snapshot.clone()),
                    value: SemanticValue::EPathMap(Box::new(summary)),
                    field_position,
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
        scores: &mut CanonicalScoreArena<'src>,
    ) -> Result<MessageSummary<'src>, CanonicalProtobufAdmissionError> {
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

fn semantic_slot<'frame, 'src>(
    frame: &'frame MessageFrame<'src>,
    name: &'static str,
) -> Result<&'frame [SemanticOccurrence<'src>], CanonicalProtobufAdmissionError> {
    let slot = frame
        .program
        .fields
        .iter()
        .position(|field| field.name == name)
        .ok_or_else(|| frame.schema_error(0, "semantic reducer field is absent from its schema"))?;
    Ok(&frame.semantic_slots[slot])
}

fn sole_occurrence<'frame, 'src>(
    frame: &'frame MessageFrame<'src>,
    name: &'static str,
) -> Result<Option<&'frame SemanticOccurrence<'src>>, CanonicalProtobufAdmissionError> {
    let values = semantic_slot(frame, name)?;
    if values.len() > 1 {
        return Err(frame.schema_error(0, "semantic singular field has multiple occurrences"));
    }
    Ok(values.first())
}

fn scalar_value<'frame, 'src>(
    frame: &'frame MessageFrame<'src>,
    name: &'static str,
) -> Result<
    Option<(&'frame SemanticScalar<'src>, Option<&'static str>)>,
    CanonicalProtobufAdmissionError,
> {
    let Some(occurrence) = sole_occurrence(frame, name)? else {
        return Ok(None);
    };
    let SemanticValue::Scalar(value) = &occurrence.value else {
        return Err(frame.schema_error(0, "semantic scalar field returned a message summary"));
    };
    Ok(Some((value, occurrence.variant)))
}

fn bool_value<'src>(
    frame: &MessageFrame<'src>,
    name: &'static str,
) -> Result<bool, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(false),
        Some((SemanticScalar::Bool(value), _)) => Ok(*value),
        Some(_) => Err(frame.schema_error(0, "semantic bool field has a different scalar type")),
    }
}

fn i32_value<'src>(
    frame: &MessageFrame<'src>,
    name: &'static str,
) -> Result<i32, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(0),
        Some((SemanticScalar::I32(value), _)) => Ok(*value),
        Some(_) => Err(frame.schema_error(0, "semantic i32 field has a different scalar type")),
    }
}

fn u32_value<'src>(
    frame: &MessageFrame<'src>,
    name: &'static str,
) -> Result<u32, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(0),
        Some((SemanticScalar::U32(value), _)) => Ok(*value),
        Some(_) => Err(frame.schema_error(0, "semantic u32 field has a different scalar type")),
    }
}

fn u64_value<'src>(
    frame: &MessageFrame<'src>,
    name: &'static str,
) -> Result<u64, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(0),
        Some((SemanticScalar::U64(value), _)) => Ok(*value),
        Some(_) => Err(frame.schema_error(0, "semantic u64 field has a different scalar type")),
    }
}

fn bytes_value<'src>(
    frame: &MessageFrame<'src>,
    name: &'static str,
) -> Result<Option<AdmissionRegion<'src>>, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(None),
        Some((SemanticScalar::Bytes(value), _)) => Ok(Some(value.clone())),
        Some(_) => Err(frame.schema_error(0, "semantic bytes field has a different scalar type")),
    }
}

fn string_value<'src>(
    frame: &MessageFrame<'src>,
    name: &'static str,
) -> Result<Option<AdmissionRegion<'src>>, CanonicalProtobufAdmissionError> {
    match scalar_value(frame, name)? {
        None => Ok(None),
        Some((SemanticScalar::String(value), _)) => Ok(Some(value.clone())),
        Some(_) => Err(frame.schema_error(0, "semantic string field has a different scalar type")),
    }
}

fn message_summary<'frame, 'src>(
    occurrence: &'frame SemanticOccurrence<'src>,
    frame: &MessageFrame<'src>,
) -> Result<&'frame MessageSummary<'src>, CanonicalProtobufAdmissionError> {
    let SemanticValue::Message(summary) = &occurrence.value else {
        return Err(frame.schema_error(0, "semantic message field returned a scalar summary"));
    };
    Ok(summary)
}

fn par_occurrence<'frame, 'src>(
    occurrence: &'frame SemanticOccurrence<'src>,
    frame: &MessageFrame<'src>,
) -> Result<
    (&'frame ParSummary<'src>, &'frame AdmissionRegion<'src>),
    CanonicalProtobufAdmissionError,
> {
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

fn optional_par<'frame, 'src>(
    frame: &'frame MessageFrame<'src>,
    name: &'static str,
) -> Result<
    Option<(&'frame ParSummary<'src>, &'frame AdmissionRegion<'src>)>,
    CanonicalProtobufAdmissionError,
> {
    sole_occurrence(frame, name)?
        .map(|occurrence| par_occurrence(occurrence, frame))
        .transpose()
}

fn optional_var<'src>(
    frame: &MessageFrame<'src>,
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

fn cost_score<'src>(
    frame: &MessageFrame<'src>,
    name: &'static str,
) -> Result<Option<CanonicalScoreHandle>, CanonicalProtobufAdmissionError> {
    let Some(occurrence) = sole_occurrence(frame, name)? else {
        return Ok(None);
    };
    let summary = message_summary(occurrence, frame)?;
    let SemanticSummary::CostScore(score) = summary.semantic else {
        return Err(frame.schema_error(0, "cost message returned a different semantic summary"));
    };
    Ok(Some(score))
}

fn empty_par_score<'src>(
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<CanonicalScoreHandle, CanonicalProtobufAdmissionError> {
    Ok(semantics::par_score(scores, Vec::new(), false)?)
}

fn nonsemantic_score<'src>(
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<CanonicalScoreHandle, CanonicalProtobufAdmissionError> {
    Ok(semantics::nonsemantic_score(scores)?)
}

fn reduce_message<'src>(
    frame: &mut MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<MessageSummary<'src>, CanonicalProtobufAdmissionError> {
    let kind = semantic_kind(frame.program)
        .ok_or_else(|| frame.schema_error(0, "message has no canonical semantic reducer"))?;
    let semantic = reduce_semantic_kind(frame, scores, kind)?;
    Ok(MessageSummary { semantic })
}

fn insert_semantic_par_identity<'src>(seen: &mut PathMap<()>, par: &ParSummary<'src>) -> bool {
    super::canonical_par_identity::replace_semantic_par_identity(
        seen,
        par.identity_prefix
            .bytes()
            .chain(par.identity_suffix.bytes()),
        (),
    )
    .is_none()
}

fn reduce_semantic_kind<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
    kind: SemanticKind,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    match kind {
        SemanticKind::Opaque => Ok(SemanticSummary::Opaque),
        SemanticKind::RawPar => reduce_raw_par(frame, scores),
        SemanticKind::Component(family) => reduce_component(frame, scores, family),
        SemanticKind::Var => reduce_var(frame, scores),
        SemanticKind::Unit => Ok(SemanticSummary::Unit),
        SemanticKind::ReceiveBind => reduce_receive_bind(frame, scores),
        SemanticKind::CostSignature => reduce_cost_signature(frame, scores),
        SemanticKind::CostSignatureAtom => reduce_cost_signature_atom(frame, scores),
        SemanticKind::CostBoundSignature => Ok(SemanticSummary::CostScore(
            semantics::cost_bound_score(scores, i32_value(frame, "index")?)?,
        )),
        SemanticKind::CostRegion => reduce_cost_region(frame, scores),
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

fn empty_region<'src>(region: &AdmissionRegion<'src>) -> AdmissionRegion<'src> {
    region
        .subview(CanonicalByteRange::default())
        .expect("an empty subregion is inside every protobuf region")
}

fn occurrence_scalar<'frame, 'src>(
    frame: &MessageFrame<'src>,
    occurrence: &'frame SemanticOccurrence<'src>,
) -> Result<&'frame SemanticScalar<'src>, CanonicalProtobufAdmissionError> {
    let SemanticValue::Scalar(value) = &occurrence.value else {
        return Err(frame.schema_error(0, "oneof scalar arm returned a message summary"));
    };
    Ok(value)
}

fn reduce_var<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
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

fn reduce_cost_signature<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let mut atoms = Vec::new();
    for occurrence in semantic_slot(frame, "atoms")? {
        let summary = message_summary(occurrence, frame)?;
        let SemanticSummary::CostScore(score) = summary.semantic else {
            return Err(frame.schema_error(0, "cost atom returned a different semantic summary"));
        };
        atoms.push(score);
    }
    Ok(SemanticSummary::CostScore(semantics::cost_signature_score(
        scores, atoms,
    )?))
}

fn reduce_cost_signature_atom<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let score = match sole_occurrence(frame, "form")? {
        Some(occurrence) => match occurrence.variant {
            Some("Ground") => {
                let SemanticValue::Scalar(SemanticScalar::Bytes(bytes)) = &occurrence.value else {
                    return Err(frame.schema_error(0, "cost ground is not bytes"));
                };
                semantics::cost_ground_score(scores, &bytes.bytes().collect::<Vec<_>>())?
            }
            Some("Quote") => {
                let SemanticValue::Scalar(SemanticScalar::Bytes(bytes)) = &occurrence.value else {
                    return Err(frame.schema_error(0, "cost quote is not bytes"));
                };
                semantics::cost_quote_score(scores, &bytes.bytes().collect::<Vec<_>>())?
            }
            Some("RuntimeName") => {
                let SemanticValue::Scalar(SemanticScalar::Bytes(bytes)) = &occurrence.value else {
                    return Err(frame.schema_error(0, "cost runtime name is not bytes"));
                };
                semantics::cost_runtime_name_score(scores, &bytes.bytes().collect::<Vec<_>>())?
            }
            Some("Bound") => {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::CostScore(score) = summary.semantic else {
                    return Err(frame.schema_error(0, "cost bound returned a different summary"));
                };
                score
            }
            _ => return Err(frame.schema_error(0, "unknown cost signature atom form")),
        },
        None => semantics::cost_form_absent_score(scores)?,
    };
    Ok(SemanticSummary::CostScore(score))
}

fn reduce_cost_region<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let version = u32_value(frame, "version")?;
    let coordinates = [
        u64_value(frame, "start_line")?,
        u64_value(frame, "start_column")?,
        u64_value(frame, "end_line")?,
        u64_value(frame, "end_column")?,
    ];
    let signer = cost_score(frame, "signer")?;
    let ordinal = u32_value(frame, "ordinal")?;
    Ok(SemanticSummary::CostScore(
        semantics::cost_region_score_parts(scores, version, coordinates, signer, ordinal)?,
    ))
}

fn reduce_receive_bind<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let source = optional_par(frame, "source")?;
    let source_present = source.is_some();
    let score = if let Some((source, _)) = source {
        let mut patterns = Vec::new();
        for occurrence in semantic_slot(frame, "patterns")? {
            patterns.push(par_occurrence(occurrence, frame)?.0.score);
        }
        let remainder =
            optional_var(frame, "remainder")?.unwrap_or(scores.integer(i64::from(Score::ABSENT))?);
        let region = cost_score(frame, "cost_region")?;
        Some(semantics::receive_bind_score(
            scores,
            source.score,
            patterns,
            remainder,
            region,
        )?)
    } else {
        None
    };
    Ok(SemanticSummary::ReceiveBind {
        score,
        source_present,
    })
}

fn reduce_match_case<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
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

fn reduce_expr_var<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
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

fn reduce_expr_unary<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
    constructor: i32,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
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

fn reduce_expr_binary<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
    constructor: i32,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
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

fn reduce_key_value_pair<'src>(
    frame: &MessageFrame<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    Ok(SemanticSummary::KeyValuePair {
        key: optional_par(frame, "key")?.map(|(par, _)| par.clone()),
        value: optional_par(frame, "value")?.map(|(par, _)| par.clone()),
    })
}

fn reduce_raw_par<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
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
                    occurrence.field_position,
                    "raw Par family returned a different semantic component",
                ));
            };
            if component.family != family {
                return Err(frame.schema_error_at(
                    u32::from(family.byte()),
                    occurrence.field_position,
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
        .subview(prefix_range)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let identity_suffix = frame
        .body
        .subview(suffix_range)
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

fn component_summary<'src>(
    family: ParComponentTag,
    score: CanonicalScoreHandle,
    complete: bool,
    canonical_path_stable: bool,
) -> SemanticSummary<'src> {
    SemanticSummary::Component(ComponentSummary {
        family,
        score,
        complete,
        canonical_path_stable,
    })
}

fn fixed_point_error<'src>(
    frame: &MessageFrame<'src>,
    detail: &'static str,
) -> CanonicalProtobufAdmissionError {
    frame.schema_error_at(0, frame.region_start, detail)
}

fn reduce_component<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
    family: ParComponentTag,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let (complete, score) = match family {
        ParComponentTag::Send => reduce_component_send(frame, scores),
        ParComponentTag::Receive => reduce_component_receive(frame, scores),
        ParComponentTag::New => reduce_component_new(frame, scores),
        ParComponentTag::Match => reduce_component_match(frame, scores),
        ParComponentTag::Bundle => reduce_component_bundle(frame, scores),
        ParComponentTag::If => reduce_component_if(frame, scores),
        ParComponentTag::CostTokenStack => reduce_component_cost_token_stack(frame, scores),
        ParComponentTag::Expr | ParComponentTag::Connective | ParComponentTag::GUnforgeable => {
            reduce_component_specialized(frame, family)
        }
    }?;
    Ok(component_summary(family, score, complete, false))
}

fn reduce_component_cost_token_stack<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    let mut layers = Vec::new();
    for occurrence in semantic_slot(frame, "layers")? {
        let summary = message_summary(occurrence, frame)?;
        let SemanticSummary::CostScore(score) = summary.semantic else {
            return Err(
                frame.schema_error(0, "cost stack layer returned a different semantic summary")
            );
        };
        layers.push(score);
    }
    Ok((
        true,
        semantics::cost_token_stack_score_parts(scores, layers)?,
    ))
}

fn reduce_component_send<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    Ok({
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
                    cost_score(frame, "cost_region")?,
                )?,
            )
        }
    })
}

fn reduce_component_receive<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    Ok({
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
                    occurrence.field_position,
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
                    cost_score(frame, "cost_region")?,
                )?,
            )
        }
    })
}

fn reduce_component_new<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    Ok({
        let body = optional_par(frame, "p")?;
        if body.is_none() {
            (false, nonsemantic_score(scores)?)
        } else {
            let uris = semantic_slot(frame, "uri")?;
            let mut uri_scores = Vec::new();
            let mut previous_uri: Option<AdmissionRegion<'src>> = None;
            for occurrence in uris {
                let SemanticValue::Scalar(SemanticScalar::String(uri)) = &occurrence.value else {
                    return Err(frame.schema_error_at(
                        3,
                        occurrence.field_position,
                        "New URI returned a different semantic scalar",
                    ));
                };
                if previous_uri
                    .as_ref()
                    .is_some_and(|previous| previous.compare(uri).is_gt())
                {
                    return Err(frame.schema_error_at(
                        3,
                        occurrence.field_position,
                        "New URIs are outside canonical string order",
                    ));
                }
                previous_uri = Some(uri.clone());
                uri_scores.push(scores.indexed_string(uri.clone())?.into_score());
            }
            if uri_scores.is_empty() {
                uri_scores.push(scores.integer(i64::from(Score::ABSENT))?);
            }

            let mut injection_scores = Vec::new();
            for occurrence in semantic_slot(frame, "injections")? {
                let SemanticValue::Map(entry) = &occurrence.value else {
                    return Err(frame.schema_error_at(
                        4,
                        occurrence.field_position,
                        "New injection returned a different semantic summary",
                    ));
                };
                let value_score = match &entry.value {
                    Some(value) => value.score,
                    None => empty_par_score(scores)?,
                };
                let key = scores.indexed_string(entry.key.clone())?;
                injection_scores.push(semantics::injection_score_pre_scored(
                    scores,
                    key,
                    value_score,
                )?);
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
    })
}

fn reduce_component_match<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    Ok({
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
                    occurrence.field_position,
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
    })
}

fn reduce_component_bundle<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    Ok({
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
    })
}

fn reduce_component_if<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    Ok({
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
    })
}

fn reduce_component_specialized(
    frame: &MessageFrame<'_>,
    family: ParComponentTag,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    Err(frame.schema_error(
        u32::from(family.byte()),
        "component reducer received a specialized component family",
    ))
}

fn reduce_expr<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
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
                let value = scores.indexed_string(value.clone())?;
                (
                    semantics::ground_score_pre_scored(
                        scores,
                        PreScoredGroundScore::String(value),
                    )?,
                    true,
                    true,
                    true,
                )
            }
            (Some("GUri"), SemanticValue::Scalar(SemanticScalar::String(value))) => {
                let value = scores.indexed_string(value.clone())?;
                (
                    semantics::ground_score_pre_scored(scores, PreScoredGroundScore::Uri(value))?,
                    true,
                    true,
                    true,
                )
            }
            (Some("GByteArray"), SemanticValue::Scalar(SemanticScalar::Bytes(value))) => {
                let value = scores.indexed_bytes(value.clone())?;
                (
                    semantics::ground_score_pre_scored(
                        scores,
                        PreScoredGroundScore::ByteArray(value),
                    )?,
                    true,
                    true,
                    true,
                )
            }
            (Some("GDouble"), SemanticValue::Scalar(SemanticScalar::U64(value))) => (
                semantics::ground_score(scores, semantics::GroundScore::Double(*value))?,
                true,
                true,
                true,
            ),
            (Some("GBigInt"), SemanticValue::Scalar(SemanticScalar::Bytes(value))) => {
                let value = scores.indexed_bytes(value.clone())?;
                (
                    semantics::ground_score_pre_scored(
                        scores,
                        PreScoredGroundScore::BigInt(value),
                    )?,
                    true,
                    true,
                    true,
                )
            }
            (Some("GBigRat"), SemanticValue::Message(summary)) => {
                let SemanticSummary::BigRational {
                    numerator,
                    denominator,
                } = &summary.semantic
                else {
                    return Err(frame.schema_error_at(
                        36,
                        occurrence.field_position,
                        "GBigRat returned a different semantic summary",
                    ));
                };
                let numerator = scores.indexed_bytes(numerator.clone())?;
                let denominator = scores.indexed_bytes(denominator.clone())?;
                (
                    semantics::ground_score_pre_scored(scores, PreScoredGroundScore::BigRat {
                        numerator,
                        denominator,
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
                        occurrence.field_position,
                        "GFixedPoint returned a different semantic summary",
                    ));
                };
                let unscaled = scores.indexed_bytes(unscaled.clone())?;
                (
                    semantics::ground_score_pre_scored(scores, PreScoredGroundScore::FixedPoint {
                        unscaled,
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
                        occurrence.field_position,
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
                    occurrence.field_position,
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

fn reduce_connective<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let score = match sole_occurrence(frame, "connective_instance")? {
        None => semantics::connective_score(scores, semantics::ConnectiveScore::Absent)?,
        Some(occurrence) => match occurrence.variant {
            Some("ConnAndBody") | Some("ConnOrBody") => {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::ConnectiveBody(pars) = &summary.semantic else {
                    return Err(frame.schema_error_at(
                        1,
                        occurrence.field_position,
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
                        occurrence.field_position,
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
                        occurrence.field_position,
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

fn reduce_unforgeable<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let (descriptor, stable) = match sole_occurrence(frame, "unf_instance")? {
        None => (PreScoredUnforgeableScore::Absent, false),
        Some(occurrence) => match occurrence.variant {
            Some("GPrivateBody") => {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::Bytes(value) = &summary.semantic else {
                    return Err(frame.schema_error_at(
                        1,
                        occurrence.field_position,
                        "GPrivate returned a different semantic summary",
                    ));
                };
                (
                    PreScoredUnforgeableScore::Private(scores.indexed_bytes(value.clone())?),
                    true,
                )
            }
            Some("GDeployIdBody") => {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::Bytes(value) = &summary.semantic else {
                    return Err(frame.schema_error_at(
                        2,
                        occurrence.field_position,
                        "GDeployId returned a different semantic summary",
                    ));
                };
                (
                    PreScoredUnforgeableScore::DeployId(scores.indexed_bytes(value.clone())?),
                    false,
                )
            }
            Some("GDeployerIdBody") => {
                let summary = message_summary(occurrence, frame)?;
                let SemanticSummary::Bytes(value) = &summary.semantic else {
                    return Err(frame.schema_error_at(
                        3,
                        occurrence.field_position,
                        "GDeployerId returned a different semantic summary",
                    ));
                };
                (
                    PreScoredUnforgeableScore::DeployerId(scores.indexed_bytes(value.clone())?),
                    false,
                )
            }
            Some("GSysAuthTokenBody") => (PreScoredUnforgeableScore::SysAuthToken, false),
            _ => return Err(frame.schema_error(1, "unknown GUnforgeable oneof arm")),
        },
    };
    let score = semantics::unforgeable_score_pre_scored(scores, descriptor)?;
    Ok(component_summary(
        ParComponentTag::GUnforgeable,
        score,
        true,
        stable,
    ))
}

fn reduce_ordered_collection<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
    constructor: i32,
    has_remainder: bool,
    canonical_set: bool,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let occurrences = semantic_slot(frame, "ps")?;
    let mut element_scores = Vec::with_capacity(occurrences.len());
    let mut all_stable = true;
    let mut canonical_fixed_point = true;
    let mut previous: Option<(&ParSummary<'src>, &AdmissionRegion<'src>)> = None;
    let mut identities = canonical_set.then(PathMap::new);
    for occurrence in occurrences {
        let (par, body) = par_occurrence(occurrence, frame)?;
        element_scores.push(par.score);
        all_stable &= par.canonical_path_stable;
        if canonical_set {
            if let Some((previous_par, previous_body)) = previous {
                let order = scores
                    .compare(previous_par.score, par.score)
                    .then_with(|| previous_body.compare(body));
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

fn reduce_emap<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let occurrences = semantic_slot(frame, "kvs")?;
    let mut pair_scores = Vec::with_capacity(occurrences.len());
    let mut complete = true;
    let mut canonical_fixed_point = true;
    let mut previous: Option<(&ParSummary<'src>, &AdmissionRegion<'src>)> = None;
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
                .then_with(|| previous_pair_body.compare(pair_body));
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

fn reduce_epathmap<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let occurrence = sole_occurrence(frame, "entries")?
        .ok_or_else(|| frame.schema_error(9, "EPathMap snapshot semantic summary is absent"))?;
    let SemanticValue::EPathMap(snapshot) = &occurrence.value else {
        return Err(frame.schema_error(9, "EPathMap snapshot returned the wrong semantic summary"));
    };
    let snapshot_score = scores
        .indexed_bytes(snapshot.snapshot.clone())?
        .into_score();
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

fn reduce_ezipper<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
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
    let snapshot_score = scores.indexed_bytes(snapshot.clone())?.into_score();
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

fn reduce_emethod<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let target = optional_par(frame, "target")?;
    let score = if let Some((target, _)) = target {
        let mut arguments = Vec::new();
        for occurrence in semantic_slot(frame, "arguments")? {
            arguments.push(par_occurrence(occurrence, frame)?.0.score);
        }
        let method =
            string_value(frame, "method_name")?.unwrap_or_else(|| empty_region(&frame.body));
        let method = scores.indexed_string(method)?;
        Some(semantics::method_score_pre_scored(
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

struct MapFrame<'src> {
    owner: &'static RhoMatchProgram,
    outer_field: u32,
    value_type_id: u16,
    region: AdmissionRegion<'src>,
    region_start: CanonicalByteProvenance,
    cursor: IndexedCanonicalFieldCursor<'src>,
    key: Option<AdmissionRegion<'src>>,
    key_position: Option<CanonicalByteProvenance>,
    saw_value: bool,
    value_position: Option<CanonicalByteProvenance>,
    value: Option<ParSummary<'src>>,
    waiting_value: bool,
}

impl<'src> MapFrame<'src> {
    fn from_map_region(
        owner: &'static RhoMatchProgram,
        outer_field: u32,
        value_type_id: u16,
        region: AdmissionRegion<'src>,
    ) -> Self {
        let region_start = region
            .position_at(0)
            .expect("indexed regions own their start")
            .physical;
        Self {
            owner,
            outer_field,
            value_type_id,
            cursor: IndexedCanonicalFieldCursor::new(region.clone()),
            region,
            region_start,
            key: None,
            key_position: None,
            saw_value: false,
            value_position: None,
            value: None,
            waiting_value: false,
        }
    }

    fn schema_error(&self, detail: &'static str) -> CanonicalProtobufAdmissionError {
        self.schema_error_at(self.outer_field, self.region_start, detail)
    }

    fn schema_error_at(
        &self,
        field: u32,
        position: CanonicalByteProvenance,
        detail: &'static str,
    ) -> CanonicalProtobufAdmissionError {
        CanonicalProtobufAdmissionError::Schema {
            type_name: "protobuf map entry",
            field,
            position,
            detail,
        }
    }

    fn field_error(
        &self,
        field: &IndexedCanonicalField<'src>,
        detail: &'static str,
    ) -> CanonicalProtobufAdmissionError {
        self.schema_error_at(field.number, field.field_start.physical, detail)
    }

    fn accept(
        &mut self,
        summary: FrameSummary<'src>,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
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
                self.value_position.unwrap_or(self.region_start),
                "explicit protobuf map value equals the omitted default",
            ));
        }
        self.value = Some(value);
        Ok(())
    }

    fn finish(&mut self) -> Result<MapSummary<'src>, CanonicalProtobufAdmissionError> {
        if self.waiting_value {
            return Err(self.schema_error("map entry completed with a pending value"));
        }
        let key = self.key.clone().unwrap_or_else(|| {
            self.region
                .subview(CanonicalByteRange::default())
                .expect("an empty range is inside every map entry")
        });
        Ok(MapSummary {
            key,
            key_position: self.key_position.unwrap_or(self.region_start),
            value: self.value.take(),
        })
    }
}

struct EPathMapFrame<'src> {
    owner: &'static RhoMatchProgram,
    field: u32,
    region_start: CanonicalByteProvenance,
    snapshot: AdmissionRegion<'src>,
    cursor: IndexedCanonicalEpm1TopologyCursor<'src>,
    path: CanonicalPathTopologyAdmission<'src>,
    mode: EPathMapMode,
    entries_canonical_path_stable: bool,
    waiting: Option<EPathMapWait>,
    last_source: CanonicalPathSource,
}

enum EPathMapWait {
    MapValue,
    Obligation(PathObligationWait),
}

struct PathObligationWait {
    id: CanonicalPathObligationId,
    kind: CanonicalPathObligationKind,
}

impl PathObligationWait {
    fn resolution(&self, summary: FrameSummary<'_>) -> Option<CanonicalPathObligationResolution> {
        match (self.kind, summary) {
            (
                CanonicalPathObligationKind::RawParEscape,
                FrameSummary::Message(MessageSummary {
                    semantic: SemanticSummary::Par(par),
                }),
            ) => Some(CanonicalPathObligationResolution::RawPar {
                canonical_path_stable: par.canonical_path_stable,
            }),
            (CanonicalPathObligationKind::NestedEpm1, FrameSummary::EPathMap(epm)) => {
                Some(CanonicalPathObligationResolution::NestedEpm1 {
                    mode: match epm.mode {
                        EPathMapMode::Empty => NestedEpmMode::Empty,
                        EPathMapMode::Set => NestedEpmMode::Set,
                        EPathMapMode::Map => NestedEpmMode::Map,
                    },
                    entries_stable: epm.entries_canonical_path_stable,
                })
            }
            _ => None,
        }
    }
}

fn obligation_frame<'src>(
    completed: CanonicalPathCompletedObligation<SourceTreeWindow<'src>>,
    owner: &'static RhoMatchProgram,
    field: u32,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<(PathObligationWait, FrameRequest<'src>), CanonicalProtobufAdmissionError> {
    let end =
        CanonicalByteProvenance::new(completed.payload_end.source, completed.payload_end.offset);
    let body = IndexedByteRegion::from_source(completed.payload, end);
    obligation_region_frame(completed.obligation, body, owner, field, stats)
}

fn obligation_region_frame<'src>(
    obligation: CanonicalPathObligation,
    body: AdmissionRegion<'src>,
    owner: &'static RhoMatchProgram,
    field: u32,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<(PathObligationWait, FrameRequest<'src>), CanonicalProtobufAdmissionError> {
    let payload_len = body.len();
    stats.canonical_path_obligations = stats
        .canonical_path_obligations
        .checked_add(1)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    stats.canonical_path_payload_bytes = stats
        .canonical_path_payload_bytes
        .checked_add(payload_len)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let waiting = PathObligationWait {
        id: obligation.id,
        kind: obligation.kind,
    };
    let frame = match waiting.kind {
        CanonicalPathObligationKind::RawParEscape => FrameRequest::Message {
            region: body,
            program: &PAR_PROTOBUF_REGION_PROGRAM,
        },
        CanonicalPathObligationKind::NestedEpm1 => FrameRequest::EPathMap {
            region: body,
            owner,
            field,
        },
    };
    Ok((waiting, frame))
}

struct CanonicalPathFrame<'src> {
    input: AdmissionRegion<'src>,
    cursor: IndexedLocatedByteCursor<'src>,
    reducer: CanonicalPathStreamReducer,
    waiting: Option<PathObligationWait>,
}

impl<'src> CanonicalPathFrame<'src> {
    fn new(input: AdmissionRegion<'src>) -> Self {
        let reducer = CanonicalPathStreamReducer::new();
        Self {
            cursor: input.located_bytes(),
            input,
            reducer,
            waiting: None,
        }
    }

    fn path_error(
        source: super::canonical_path_admission::CanonicalPathAdmissionError,
    ) -> CanonicalProtobufAdmissionError {
        CanonicalProtobufAdmissionError::CanonicalPath {
            type_name: "CanonicalPath",
            field: 0,
            position: source.source,
            detail: source.to_string(),
        }
    }

    fn protocol_error(&self, detail: &'static str) -> CanonicalProtobufAdmissionError {
        CanonicalProtobufAdmissionError::Schema {
            type_name: "CanonicalPath",
            field: 0,
            position: self.input.end_position().physical,
            detail,
        }
    }

    fn accept(
        &mut self,
        summary: FrameSummary<'src>,
        stats: &mut CanonicalProtobufAdmissionStats,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        let waiting = self.waiting.take().ok_or_else(|| {
            self.protocol_error("canonical path child returned without a continuation")
        })?;
        let resolution = waiting.resolution(summary).ok_or_else(|| {
            self.protocol_error("canonical path child returned the wrong summary kind")
        })?;
        self.reducer
            .resolve_obligation(&self.input, waiting.id, resolution)
            .map_err(Self::path_error)?;
        stats.canonical_path_resolutions = stats
            .canonical_path_resolutions
            .checked_add(1)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        Ok(())
    }

    fn step(
        &mut self,
        stats: &mut CanonicalProtobufAdmissionStats,
    ) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
        if self.waiting.is_some() {
            return Err(self.protocol_error("canonical path resumed before its child returned"));
        }
        while let Some(byte) = self.cursor.next() {
            let transition = self
                .reducer
                .edge(&self.input, byte)
                .map_err(Self::path_error)?;
            if matches!(transition, CanonicalPathTransition::ObligationStarted(_)) {
                if let Some(plan) = self.reducer.plan_opaque_interior(self.input.len()) {
                    assert!(
                        self.reducer
                            .try_advance_opaque_interior(self.input.len(), plan),
                        "a fresh opaque interior plan remains valid without intervening mutation"
                    );
                    assert!(
                        self.cursor.skip_exact(plan.width),
                        "the parent cursor and reducer consume the same input"
                    );
                }
            }
            if let Some(completed) = self
                .reducer
                .completed_obligation(&self.input)
                .map_err(Self::path_error)?
            {
                let body = self
                    .input
                    .subview_with_end(
                        completed.payload,
                        CanonicalByteProvenance::new(
                            completed.payload_end.source,
                            completed.payload_end.offset,
                        ),
                    )
                    .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
                let (waiting, frame) = obligation_region_frame(
                    completed.obligation,
                    body,
                    &PAR_PROTOBUF_REGION_PROGRAM,
                    0,
                    stats,
                )?;
                self.waiting = Some(waiting);
                return Ok(FrameAction::Push(frame));
            }
        }
        let end = self.input.end_position().physical;
        let summary = self
            .reducer
            .value(true, CanonicalPathSource::in_source(end.source, end.offset))
            .expect("standalone canonical path owns one endpoint")
            .map_err(Self::path_error)?;
        stats.canonical_path_bytes_read = stats
            .canonical_path_bytes_read
            .checked_add(self.cursor.bytes_yielded())
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        stats.canonical_path_bytes_traversed = stats
            .canonical_path_bytes_traversed
            .checked_add(self.cursor.bytes_traversed())
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        Ok(FrameAction::Complete(FrameSummary::CanonicalPath(summary)))
    }
}

impl<'src> EPathMapFrame<'src> {
    fn with_epm1_region<T>(
        owner: &'static RhoMatchProgram,
        field: u32,
        snapshot: AdmissionRegion<'src>,
        finish: impl FnOnce(Self) -> Result<T, CanonicalProtobufAdmissionError>,
    ) -> Result<T, CanonicalProtobufAdmissionError> {
        let start = snapshot
            .position_at(0)
            .expect("indexed EPM1 owns its start");
        let region_start = start.physical;
        let cursor =
            IndexedCanonicalEpm1TopologyCursor::new(snapshot.clone()).map_err(|source| {
                CanonicalProtobufAdmissionError::EPathMap {
                    type_name: owner.type_name,
                    field,
                    position: region_start,
                    source,
                }
            })?;
        let mode = cursor.mode();
        let path = CanonicalPathTopologyAdmission::new_with_source(cursor.source_window())
            .map_err(|_| CanonicalProtobufAdmissionError::SizeOverflow)?;
        finish(Self {
            owner,
            field,
            region_start,
            snapshot,
            cursor,
            path,
            mode,
            entries_canonical_path_stable: true,
            waiting: None,
            last_source: CanonicalPathSource::in_source(
                start.physical.source,
                start.physical.offset,
            ),
        })
    }

    fn path_error(
        &self,
        source: super::canonical_path_admission::CanonicalPathAdmissionError,
    ) -> CanonicalProtobufAdmissionError {
        CanonicalProtobufAdmissionError::CanonicalPath {
            type_name: self.owner.type_name,
            field: self.field,
            position: source.source,
            detail: source.to_string(),
        }
    }
}

impl<'src> EPathMapFrame<'src> {
    fn accept(
        &mut self,
        summary: FrameSummary<'src>,
        stats: &mut CanonicalProtobufAdmissionStats,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        let Some(waiting) = self.waiting.take() else {
            return Err(CanonicalProtobufAdmissionError::Schema {
                type_name: self.owner.type_name,
                field: self.field,
                position: self.region_start,
                detail: "EPM1 value returned without a continuation",
            });
        };
        match (waiting, summary) {
            (
                EPathMapWait::MapValue,
                FrameSummary::Message(MessageSummary {
                    semantic: SemanticSummary::Par(par),
                }),
            ) => {
                self.entries_canonical_path_stable &= par.canonical_path_stable;
                Ok(())
            }
            (EPathMapWait::Obligation(waiting), summary) => {
                let resolution =
                    waiting
                        .resolution(summary)
                        .ok_or(CanonicalProtobufAdmissionError::Schema {
                            type_name: self.owner.type_name,
                            field: self.field,
                            position: self.region_start,
                            detail: "EPM1 child returned the wrong summary for its continuation",
                        })?;
                self.path
                    .resolve_obligation(waiting.id, resolution)
                    .map_err(|source| self.path_error(source))?;
                stats.canonical_path_resolutions = stats
                    .canonical_path_resolutions
                    .checked_add(1)
                    .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
                Ok(())
            }
            _ => Err(CanonicalProtobufAdmissionError::Schema {
                type_name: self.owner.type_name,
                field: self.field,
                position: self.region_start,
                detail: "EPM1 child returned the wrong summary for its continuation",
            }),
        }
    }
}

enum Frame<'src> {
    Message(MessageFrame<'src>),
    Map(MapFrame<'src>),
    EPathMap(EPathMapFrame<'src>),
    CanonicalPath(CanonicalPathFrame<'src>),
}

impl<'src> Frame<'src> {
    fn step(
        &mut self,
        stats: &mut CanonicalProtobufAdmissionStats,
        scores: &mut CanonicalScoreArena<'src>,
    ) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
        match self {
            Self::Message(frame) => message_step(frame, stats, scores),
            Self::Map(frame) => map_step(frame, stats),
            Self::EPathMap(frame) => {
                let action = epathmap_step(frame, stats);
                #[cfg(test)]
                standalone_path_admission::record_full_admission_path(&frame.path);
                action
            }
            Self::CanonicalPath(frame) => frame.step(stats),
        }
    }

    fn accept(
        &mut self,
        summary: FrameSummary<'src>,
        stats: &mut CanonicalProtobufAdmissionStats,
        scores: &CanonicalScoreArena<'src>,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        match self {
            Self::Message(frame) => frame.accept(summary, scores),
            Self::Map(frame) => frame.accept(summary),
            Self::EPathMap(frame) => frame.accept(summary, stats),
            Self::CanonicalPath(frame) => frame.accept(summary, stats),
        }
    }
}

enum FrameRequest<'src> {
    Message {
        region: AdmissionRegion<'src>,
        program: &'static RhoMatchProgram,
    },
    Map {
        region: AdmissionRegion<'src>,
        owner: &'static RhoMatchProgram,
        field: u32,
        value_type_id: u16,
    },
    EPathMap {
        region: AdmissionRegion<'src>,
        owner: &'static RhoMatchProgram,
        field: u32,
    },
    CanonicalPath(AdmissionRegion<'src>),
}

impl<'src> FrameRequest<'src> {
    fn push_into(
        self,
        frames: &mut Vec<Frame<'src>>,
        stats: &mut CanonicalProtobufAdmissionStats,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        match self {
            Self::Message { region, program } => Self::push_message(frames, stats, region, program),
            Self::Map {
                region,
                owner,
                field,
                value_type_id,
            } => Self::push_map(frames, stats, owner, field, value_type_id, region),
            Self::EPathMap {
                region,
                owner,
                field,
            } => Self::push_epathmap(frames, stats, owner, field, region),
            Self::CanonicalPath(region) => {
                Self::push_canonical_path(frames, region);
                Ok(())
            }
        }
    }

    fn push_message(
        frames: &mut Vec<Frame<'src>>,
        stats: &mut CanonicalProtobufAdmissionStats,
        region: AdmissionRegion<'src>,
        program: &'static RhoMatchProgram,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        let frame = MessageFrame::from_message_region(region, program)?;
        increment_frame_count(&mut stats.messages)?;
        frames.push(Frame::Message(frame));
        Ok(())
    }

    fn push_map(
        frames: &mut Vec<Frame<'src>>,
        stats: &mut CanonicalProtobufAdmissionStats,
        owner: &'static RhoMatchProgram,
        field: u32,
        value_type_id: u16,
        region: AdmissionRegion<'src>,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        let frame = MapFrame::from_map_region(owner, field, value_type_id, region);
        increment_frame_count(&mut stats.map_entries)?;
        frames.push(Frame::Map(frame));
        Ok(())
    }

    fn push_epathmap(
        frames: &mut Vec<Frame<'src>>,
        stats: &mut CanonicalProtobufAdmissionStats,
        owner: &'static RhoMatchProgram,
        field: u32,
        region: AdmissionRegion<'src>,
    ) -> Result<(), CanonicalProtobufAdmissionError> {
        EPathMapFrame::with_epm1_region(owner, field, region, |frame| {
            increment_frame_count(&mut stats.epathmaps)?;
            frames.push(Frame::EPathMap(frame));
            Ok(())
        })
    }

    fn push_canonical_path(frames: &mut Vec<Frame<'src>>, region: AdmissionRegion<'src>) {
        frames.push(Frame::CanonicalPath(CanonicalPathFrame::new(region)));
    }
}

#[derive(Clone)]
struct MessageSummary<'src> {
    semantic: SemanticSummary<'src>,
}

#[derive(Clone)]
struct MapSummary<'src> {
    key: AdmissionRegion<'src>,
    key_position: CanonicalByteProvenance,
    value: Option<ParSummary<'src>>,
}

enum FrameSummary<'src> {
    Message(MessageSummary<'src>),
    Map(MapSummary<'src>),
    EPathMap(EPathMapSummary<'src>),
    CanonicalPath(CanonicalPathSummary),
}

enum FrameAction<'src> {
    Continue,
    Push(FrameRequest<'src>),
    Complete(FrameSummary<'src>),
}

fn canonical_wire_type(kind: ProtobufWireType) -> CanonicalWireType {
    match kind {
        ProtobufWireType::Varint => CanonicalWireType::Varint,
        ProtobufWireType::Fixed64 => CanonicalWireType::Fixed64,
        ProtobufWireType::LengthDelimited => CanonicalWireType::LengthDelimited,
        ProtobufWireType::Fixed32 => CanonicalWireType::Fixed32,
    }
}

fn verify_wire<'src>(
    frame: &MessageFrame<'src>,
    field: &IndexedCanonicalField<'src>,
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

fn scalar_error<'src>(
    frame: &MessageFrame<'src>,
    field: &IndexedCanonicalField<'src>,
    detail: &'static str,
) -> CanonicalProtobufAdmissionError {
    CanonicalProtobufAdmissionError::Schema {
        type_name: frame.program.type_name,
        field: field.number,
        position: field.field_start.physical,
        detail,
    }
}

fn framing_error(
    type_name: &'static str,
    region_start: CanonicalByteProvenance,
    source: LocatedCanonicalProtobufError,
) -> CanonicalProtobufAdmissionError {
    CanonicalProtobufAdmissionError::Framing {
        type_name,
        region_start,
        physical: source.failure,
        source: source.error,
    }
}

fn fixed_is_zero(region: &AdmissionRegion<'_>) -> bool { region.bytes().all(|byte| byte == 0) }

fn fixed_array<const N: usize>(region: &AdmissionRegion<'_>) -> [u8; N] {
    let mut bytes = [0u8; N];
    for (destination, source) in bytes.iter_mut().zip(region.bytes()) {
        *destination = source;
    }
    bytes
}

fn length_delimited<'src>(
    frame: &MessageFrame<'src>,
    field: &IndexedCanonicalField<'src>,
) -> Result<AdmissionRegion<'src>, CanonicalProtobufAdmissionError> {
    match &field.value {
        IndexedCanonicalFieldValue::LengthDelimited(value) => Ok(value.clone()),
        _ => Err(frame.field_error(field, "protobuf field is not length-delimited")),
    }
}

fn validate_scalar<'src>(
    frame: &MessageFrame<'src>,
    field: &IndexedCanonicalField<'src>,
    kind: ProtobufKind,
    presence: ScalarPresence,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let default_forbidden = !presence.permits_default();
    match (kind, &field.value) {
        (ProtobufKind::Bool, IndexedCanonicalFieldValue::Varint(value)) => {
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
        (ProtobufKind::Int32, IndexedCanonicalFieldValue::Varint(value)) => {
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
        (
            ProtobufKind::Sint32 | ProtobufKind::Uint32,
            IndexedCanonicalFieldValue::Varint(value),
        ) => {
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
            IndexedCanonicalFieldValue::Varint(value),
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
            IndexedCanonicalFieldValue::Fixed32(value),
        )
        | (ProtobufKind::Float, IndexedCanonicalFieldValue::Fixed32(value)) => {
            let zero = if kind == ProtobufKind::Float {
                let bits = u32::from_le_bytes(fixed_array(value));
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
            IndexedCanonicalFieldValue::Fixed64(value),
        )
        | (ProtobufKind::Double, IndexedCanonicalFieldValue::Fixed64(value)) => {
            let zero = if kind == ProtobufKind::Double {
                let bits = u64::from_le_bytes(fixed_array(value));
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
            IndexedCanonicalFieldValue::LengthDelimited(value),
        ) => {
            value
                .validate_utf8()
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
            IndexedCanonicalFieldValue::LengthDelimited(value),
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

fn semantic_scalar<'src>(
    field: &IndexedCanonicalField<'src>,
    kind: ProtobufKind,
) -> Result<SemanticScalar<'src>, CanonicalProtobufAdmissionError> {
    let scalar = match (kind, &field.value) {
        (ProtobufKind::Bool, IndexedCanonicalFieldValue::Varint(value)) => {
            SemanticScalar::Bool(*value != 0)
        }
        (ProtobufKind::Int32, IndexedCanonicalFieldValue::Varint(value)) => {
            SemanticScalar::I32(*value as u32 as i32)
        }
        (ProtobufKind::Sint32, IndexedCanonicalFieldValue::Varint(value)) => {
            let encoded = *value as u32;
            SemanticScalar::I32(((encoded >> 1) as i32) ^ -((encoded & 1) as i32))
        }
        (ProtobufKind::Sfixed32, IndexedCanonicalFieldValue::Fixed32(value)) => {
            SemanticScalar::I32(i32::from_le_bytes(fixed_array(value)))
        }
        (
            ProtobufKind::Uint32 | ProtobufKind::Fixed32 | ProtobufKind::Float,
            IndexedCanonicalFieldValue::Varint(value),
        ) if kind == ProtobufKind::Uint32 => SemanticScalar::U32(*value as u32),
        (
            ProtobufKind::Fixed32 | ProtobufKind::Float,
            IndexedCanonicalFieldValue::Fixed32(value),
        ) => SemanticScalar::U32(u32::from_le_bytes(fixed_array(value))),
        (ProtobufKind::Int64, IndexedCanonicalFieldValue::Varint(value)) => {
            SemanticScalar::I64(*value as i64)
        }
        (ProtobufKind::Sint64, IndexedCanonicalFieldValue::Varint(value)) => {
            SemanticScalar::I64(((value >> 1) as i64) ^ -((value & 1) as i64))
        }
        (ProtobufKind::Sfixed64, IndexedCanonicalFieldValue::Fixed64(value)) => {
            SemanticScalar::I64(i64::from_le_bytes(fixed_array(value)))
        }
        (ProtobufKind::Uint64, IndexedCanonicalFieldValue::Varint(value)) => {
            SemanticScalar::U64(*value)
        }
        (
            ProtobufKind::Fixed64 | ProtobufKind::Double,
            IndexedCanonicalFieldValue::Fixed64(value),
        ) => SemanticScalar::U64(u64::from_le_bytes(fixed_array(value))),
        (
            ProtobufKind::String | ProtobufKind::RepeatedString,
            IndexedCanonicalFieldValue::LengthDelimited(value),
        ) => SemanticScalar::String(value.clone()),
        (
            ProtobufKind::Bytes | ProtobufKind::RepeatedBytes,
            IndexedCanonicalFieldValue::LengthDelimited(value),
        ) => SemanticScalar::Bytes(value.clone()),
        _ => return Err(CanonicalProtobufAdmissionError::SizeOverflow),
    };
    Ok(scalar)
}

fn field_absolute_offset<'src>(
    frame: &MessageFrame<'src>,
    field: &IndexedCanonicalField<'src>,
) -> Result<CanonicalByteProvenance, CanonicalProtobufAdmissionError> {
    let _ = frame;
    Ok(field.field_start.physical)
}

fn record_scalar<'src>(
    frame: &mut MessageFrame<'src>,
    field: &IndexedCanonicalField<'src>,
    slot: usize,
    variant: Option<&'static str>,
    kind: ProtobufKind,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let field_position = field_absolute_offset(frame, field)?;
    let value = semantic_scalar(field, kind)?;
    if frame.is_raw_par() && frame.program.fields[slot].name == "locally_free" {
        frame.par_locally_free_range = Some(field.encoded_range.clone());
    }
    frame.semantic_slots[slot].push(SemanticOccurrence {
        variant,
        body: None,
        value: SemanticValue::Scalar(value),
        field_position,
    });
    Ok(())
}

fn child_program<'src>(
    frame: &MessageFrame<'src>,
    field: &IndexedCanonicalField<'src>,
    type_id: u16,
) -> Result<&'static RhoMatchProgram, CanonicalProtobufAdmissionError> {
    wire_program_by_type_id(type_id).map_err(|error| frame.field_error(field, error.detail()))
}

fn message_step<'src>(
    frame: &mut MessageFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
    if frame.waiting.is_some() {
        return Err(frame.schema_error(0, "protobuf message resumed before its child returned"));
    }
    let field = frame
        .cursor
        .next_field()
        .map_err(|source| framing_error(frame.program.type_name, frame.region_start, source))?;
    let Some(field) = field else {
        let cursor_stats = frame.cursor.stats();
        stats.protobuf_bytes_read = stats
            .protobuf_bytes_read
            .checked_add(cursor_stats.bytes_read)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        stats.protobuf_bytes_traversed = stats
            .protobuf_bytes_traversed
            .checked_add(cursor_stats.bytes_traversed)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        return Ok(FrameAction::Complete(FrameSummary::Message(
            frame.finish(scores)?,
        )));
    };
    message_field_step(frame, stats, field)
}

fn message_field_step<'src>(
    frame: &mut MessageFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    field: IndexedCanonicalField<'src>,
) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
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
    let field_position = field_absolute_offset(frame, &field)?;

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
                let body = length_delimited(frame, &field)?;
                frame.waiting = Some(MessageWait::Child {
                    slot,
                    variant: Some(variant.name),
                    body: body.clone(),
                    field_position,
                });
                Ok(FrameAction::Push(FrameRequest::Message {
                    region: body,
                    program: child,
                }))
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
                let body = length_delimited(frame, &field)?;
                frame.waiting = Some(MessageWait::Child {
                    slot,
                    variant: None,
                    body: body.clone(),
                    field_position,
                });
                Ok(FrameAction::Push(FrameRequest::Message {
                    region: body,
                    program: child,
                }))
            }
            RhoMatchFieldKind::StringMessageMap(type_id) => {
                let field_number = field.number;
                let body = length_delimited(frame, &field)?;
                frame.waiting = Some(MessageWait::Map {
                    slot,
                    field_position,
                });
                Ok(FrameAction::Push(FrameRequest::Map {
                    owner: frame.program,
                    field: field_number,
                    value_type_id: type_id,
                    region: body,
                }))
            }
            RhoMatchFieldKind::PathMap => {
                let field_number = field.number;
                let body = length_delimited(frame, &field)?;
                frame.waiting = Some(MessageWait::EPathMap {
                    slot,
                    field_position,
                });
                Ok(FrameAction::Push(FrameRequest::EPathMap {
                    owner: frame.program,
                    field: field_number,
                    region: body,
                }))
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

fn map_step<'src>(
    frame: &mut MapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
    if frame.waiting_value {
        return Err(frame.schema_error("map entry resumed before its value returned"));
    }
    let field = frame
        .cursor
        .next_field()
        .map_err(|source| framing_error(frame.owner.type_name, frame.region_start, source))?;
    let Some(field) = field else {
        let cursor_stats = frame.cursor.stats();
        stats.protobuf_bytes_read = stats
            .protobuf_bytes_read
            .checked_add(cursor_stats.bytes_read)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        stats.protobuf_bytes_traversed = stats
            .protobuf_bytes_traversed
            .checked_add(cursor_stats.bytes_traversed)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        return Ok(FrameAction::Complete(FrameSummary::Map(frame.finish()?)));
    };
    stats.fields = stats
        .fields
        .checked_add(1)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let field_position = field.field_start.physical;
    match field.number {
        1 if frame.key.is_none() => {
            let value = match &field.value {
                IndexedCanonicalFieldValue::LengthDelimited(value) => value.clone(),
                _ => return Err(frame.field_error(&field, "protobuf map key is not a string")),
            };
            value.validate_utf8().map_err(|_| {
                frame.schema_error_at(1, field_position, "protobuf map key is not UTF-8")
            })?;
            if value.is_empty() {
                return Err(frame.schema_error_at(
                    1,
                    field_position,
                    "explicit protobuf map key equals the omitted default",
                ));
            }
            frame.key = Some(value);
            frame.key_position = Some(field_position);
            Ok(FrameAction::Continue)
        }
        2 if !frame.saw_value => {
            let value = match &field.value {
                IndexedCanonicalFieldValue::LengthDelimited(value) => value.clone(),
                _ => return Err(frame.field_error(&field, "protobuf map value is not a message")),
            };
            let program = wire_program_by_type_id(frame.value_type_id)
                .map_err(|error| frame.schema_error_at(2, field_position, error.detail()))?;
            frame.saw_value = true;
            frame.value_position = Some(field_position);
            frame.waiting_value = true;
            Ok(FrameAction::Push(FrameRequest::Message {
                region: value,
                program,
            }))
        }
        1 | 2 => Err(frame.field_error(&field, "canonical protobuf map-entry field is duplicated")),
        _ => Err(frame.field_error(
            &field,
            "field is outside the canonical protobuf map-entry schema",
        )),
    }
}

fn try_epathmap_batch(
    frame: &mut EPathMapFrame<'_>,
) -> Result<bool, CanonicalProtobufAdmissionError> {
    #[cfg(test)]
    if standalone_path_admission::scalar_epm_transport() {
        return Ok(false);
    }
    let Some(span) = frame.cursor.line_span() else {
        return Ok(false);
    };
    let direction = span.direction();
    let width = match direction {
        CanonicalCompactTrieLineDirection::Forward => frame.path.forward_capacity(span.capacity()),
        CanonicalCompactTrieLineDirection::Unwind => frame.path.reverse_capacity(span.capacity()),
    };
    if width == 0 {
        return Ok(false);
    }
    let Some(prepared) = span.prepare(width) else {
        return Ok(false);
    };
    let accepted = match direction {
        CanonicalCompactTrieLineDirection::Forward => frame
            .path
            .try_forward_source(prepared.source_range())
            .map_err(|_| CanonicalProtobufAdmissionError::SizeOverflow)?,
        CanonicalCompactTrieLineDirection::Unwind => frame.path.try_leave_run(width).is_some(),
    };
    if !accepted {
        return Ok(false);
    }
    let source = prepared.commit();
    frame.last_source = CanonicalPathSource::in_source(source.source, source.offset);
    #[cfg(test)]
    standalone_path_admission::record_epm_batch(direction, width);
    Ok(true)
}

fn epathmap_step<'src>(
    frame: &mut EPathMapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
    if frame.waiting.is_some() {
        return Err(CanonicalProtobufAdmissionError::Schema {
            type_name: frame.owner.type_name,
            field: frame.field,
            position: frame.region_start,
            detail: "EPM1 cursor resumed before its value returned",
        });
    }
    loop {
        if try_epathmap_batch(frame)? {
            continue;
        }
        let Some(event) = frame.cursor.next_event().map_err(|source| {
            CanonicalProtobufAdmissionError::EPathMap {
                type_name: frame.owner.type_name,
                field: frame.field,
                position: frame.region_start,
                source,
            }
        })?
        else {
            frame
                .path
                .finish(frame.last_source)
                .map_err(|source| frame.path_error(source))?;
            let cursor_stats = frame.cursor.stats();
            stats.protobuf_bytes_read = stats
                .protobuf_bytes_read
                .checked_add(cursor_stats.prefix_bytes_read)
                .and_then(|count| count.checked_add(cursor_stats.value_table_bytes_read))
                .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
            stats.protobuf_bytes_traversed = stats
                .protobuf_bytes_traversed
                .checked_add(cursor_stats.prefix_bytes_traversed)
                .and_then(|count| count.checked_add(cursor_stats.value_table_bytes_traversed))
                .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
            stats.epathmap_events = stats
                .epathmap_events
                .checked_add(cursor_stats.topology_events)
                .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
            return Ok(FrameAction::Complete(FrameSummary::EPathMap(
                EPathMapSummary {
                    mode: frame.mode,
                    snapshot: frame.snapshot.clone(),
                    entries_canonical_path_stable: frame.entries_canonical_path_stable,
                },
            )));
        };
        match event {
            IndexedCanonicalEpm1TopologyEvent::Edge { byte, provenance } => {
                let source = CanonicalPathSource::in_source(provenance.source, provenance.offset);
                frame.last_source = source;
                frame
                    .path
                    .edge(LocatedPathByte::in_source(
                        byte,
                        provenance.source,
                        provenance.offset,
                    ))
                    .map_err(|source| frame.path_error(source))?;
                if let Some(completed) = frame
                    .path
                    .retained_completed_obligation()
                    .map_err(|source| frame.path_error(source))?
                {
                    let (waiting, child) =
                        obligation_frame(completed, frame.owner, frame.field, stats)?;
                    frame.waiting = Some(EPathMapWait::Obligation(waiting));
                    return Ok(FrameAction::Push(child));
                }
            }
            IndexedCanonicalEpm1TopologyEvent::Leave { provenance } => {
                let source = CanonicalPathSource::in_source(provenance.source, provenance.offset);
                frame.last_source = source;
                frame
                    .path
                    .leave(source)
                    .map_err(|source| frame.path_error(source))?;
            }
            IndexedCanonicalEpm1TopologyEvent::Value {
                has_value,
                value_body,
                provenance,
                ..
            } => {
                let source = CanonicalPathSource::in_source(provenance.source, provenance.offset);
                frame.last_source = source;
                if let Some(summary) = frame.path.value(has_value, source) {
                    let summary = summary.map_err(|source| frame.path_error(source))?;
                    frame.entries_canonical_path_stable &= summary.canonical_path_stable;
                }
                if let Some(value) = value_body {
                    frame.waiting = Some(EPathMapWait::MapValue);
                    return Ok(FrameAction::Push(FrameRequest::Message {
                        region: value,
                        program: &PAR_PROTOBUF_REGION_PROGRAM,
                    }));
                }
            }
        }
    }
}

/// Validate one exact canonical protobuf encoder image without constructing a
/// rhoapi value. The returned statistics are operational evidence only; the
/// admission proof is represented by successful completion.
fn first_score_difference<'src>(
    scores: &CanonicalScoreArena<'src>,
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

fn increment_frame_count(count: &mut usize) -> Result<(), CanonicalProtobufAdmissionError> {
    *count = count
        .checked_add(1)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    Ok(())
}

fn push_admission_frame<'src>(
    frames: &mut Vec<Frame<'src>>,
    request: FrameRequest<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<(), CanonicalProtobufAdmissionError> {
    request.push_into(frames, stats)?;
    let frame_variant_bytes = std::mem::size_of::<Frame>();
    stats.max_frames = stats.max_frames.max(frames.len());
    stats.max_frame_storage_bytes = stats.max_frame_storage_bytes.max(
        frames
            .len()
            .checked_mul(frame_variant_bytes)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?,
    );
    stats.max_frame_capacity_bytes = stats.max_frame_capacity_bytes.max(
        frames
            .capacity()
            .checked_mul(frame_variant_bytes)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?,
    );
    Ok(())
}

fn drive_admission<'src>(
    initial: FrameRequest<'src>,
    input_bytes: usize,
) -> Result<
    (
        FrameSummary<'src>,
        CanonicalScoreArena<'src>,
        CanonicalProtobufAdmissionStats,
    ),
    CanonicalProtobufAdmissionError,
> {
    let frame_variant_bytes = std::mem::size_of::<Frame>();
    let mut scores: CanonicalScoreArena<'src> = CanonicalScoreArena::new();
    let mut stats = CanonicalProtobufAdmissionStats {
        input_bytes,
        frame_variant_bytes,
        byte_native_validations: 1,
        ..CanonicalProtobufAdmissionStats::default()
    };
    let mut frames: Vec<Frame<'src>> = Vec::with_capacity(1);
    push_admission_frame(&mut frames, initial, &mut stats)?;
    loop {
        let action = frames
            .last_mut()
            .expect("canonical protobuf admission retains its root frame")
            .step(&mut stats, &mut scores)?;
        match action {
            FrameAction::Continue => {}
            FrameAction::Push(request) => push_admission_frame(&mut frames, request, &mut stats)?,
            FrameAction::Complete(summary) => {
                frames.truncate(frames.len() - 1);
                if let Some(parent) = frames.last_mut() {
                    parent.accept(summary, &mut stats, &scores)?;
                } else {
                    return Ok((summary, scores, stats));
                }
            }
        }
    }
}

fn validate_canonical_path_indexed(
    input: AdmissionRegion<'_>,
) -> Result<(CanonicalPathSummary, CanonicalProtobufAdmissionStats), CanonicalProtobufAdmissionError>
{
    let input_bytes = input.len();
    let root_end = input.end_position().physical;
    let (summary, _, mut stats) = drive_admission(FrameRequest::CanonicalPath(input), input_bytes)?;
    let FrameSummary::CanonicalPath(summary) = summary else {
        return Err(CanonicalProtobufAdmissionError::Schema {
            type_name: "CanonicalPath",
            field: 0,
            position: root_end,
            detail: "canonical path root returned the wrong summary kind",
        });
    };
    stats.protobuf_bytes_fast_forwarded = stats
        .protobuf_bytes_read
        .checked_sub(stats.protobuf_bytes_traversed)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    validate_cutover_stats(&stats)?;
    Ok((summary, stats))
}

fn validate_canonical_protobuf_inner<'src>(
    input: AdmissionRegion<'src>,
    root: &'static RhoMatchProgram,
    claimed_component: Option<(ParComponentTag, &[u8])>,
) -> Result<CanonicalProtobufAdmission, CanonicalProtobufAdmissionError> {
    let input_bytes = input.len();
    let root_start = input
        .position_at(0)
        .expect("indexed protobuf roots own their start")
        .physical;
    let (summary, scores, mut stats) = drive_admission(
        FrameRequest::Message {
            region: input,
            program: root,
        },
        input_bytes,
    )?;
    let FrameSummary::Message(summary) = summary else {
        return Err(CanonicalProtobufAdmissionError::Schema {
            type_name: root.type_name,
            field: 0,
            position: root_start,
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
        SemanticSummary::Component(component) => Some(CanonicalComponentAdmission {
            family: component.family,
            canonical_path_stable: component.canonical_path_stable,
            complete: component.complete,
        }),
        _ => None,
    };
    #[cfg(test)]
    let score_bytes = {
        let score = match &summary.semantic {
            SemanticSummary::Par(par) => Some(par.score),
            SemanticSummary::Component(component) => Some(component.score),
            SemanticSummary::Var(score) => Some(*score),
            SemanticSummary::ReceiveBind { score, .. }
            | SemanticSummary::MatchCase { score, .. } => *score,
            SemanticSummary::ExprPayload(payload) => payload.score,
            _ => None,
        };
        score.map(|score| {
            let mut cursor = scores.cursor(score);
            std::iter::from_fn(|| cursor.next()).collect()
        })
    };
    stats.protobuf_bytes_fast_forwarded = stats
        .protobuf_bytes_read
        .checked_sub(stats.protobuf_bytes_traversed)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    validate_cutover_stats(&stats)?;
    if let Some((family, claimed_score)) = claimed_component {
        let SemanticSummary::Component(root_component) = &summary.semantic else {
            return Err(CanonicalProtobufAdmissionError::Schema {
                type_name: root.type_name,
                field: 0,
                position: root_start,
                detail: "claimed component score was supplied for a non-component root",
            });
        };
        if root_component.family != family {
            return Err(CanonicalProtobufAdmissionError::Schema {
                type_name: root.type_name,
                field: u32::from(family.byte()),
                position: root_start,
                detail: "claimed component family disagrees with the schema root",
            });
        }
        if let Some(first_difference) =
            first_score_difference(&scores, root_component.score, claimed_score)
        {
            return Err(CanonicalProtobufAdmissionError::ScoreMismatch {
                type_name: root.type_name,
                region_start: root_start,
                first_difference,
            });
        }
    }
    Ok(CanonicalProtobufAdmission {
        stats,
        semantic_default_par,
        canonical_path_stable_par,
        component,
        #[cfg(test)]
        score_bytes,
    })
}

/// The executable no-fallback verdict shared by production completion and its
/// mutation proof. Keeping it separate makes every forbidden legacy route
/// independently falsifiable without weakening the successful fast path.
fn validate_cutover_stats(
    stats: &CanonicalProtobufAdmissionStats,
) -> Result<(), CanonicalProtobufAdmissionError> {
    if stats.reconstructed_key_bytes != 0 {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "reconstructed key bytes are nonzero",
        ));
    }
    if stats.legacy_endpoint_replays != 0 {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "legacy endpoint replays are nonzero",
        ));
    }
    if stats.typed_decodes != 0 {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "typed decodes are nonzero",
        ));
    }
    if stats.byte_native_validations != 1 {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "byte-native validation count is not exactly one",
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn validate_canonical_protobuf(
    input: ProtobufRegion,
    root: &'static RhoMatchProgram,
) -> Result<CanonicalProtobufAdmission, CanonicalProtobufAdmissionError> {
    let indexed = IndexedByteRegion::from_protobuf_region(input, 0);
    validate_canonical_protobuf_inner(indexed, root, None)
}

#[cfg(test)]
pub(crate) fn validate_canonical_component_protobuf(
    input: ProtobufRegion,
    root: &'static RhoMatchProgram,
    family: ParComponentTag,
    claimed_score: &[u8],
) -> Result<CanonicalComponentAdmission, CanonicalProtobufAdmissionError> {
    let indexed = IndexedByteRegion::from_protobuf_region(input, 0);
    validate_canonical_component_protobuf_indexed(indexed, root, family, claimed_score)
}

/// Validate one component directly from an indexed source. Borrowed resident
/// RPB3 keys use this entry point so admission neither copies the protobuf
/// payload nor decodes and re-encodes an AST merely to prove canonicality.
pub(crate) fn validate_canonical_component_protobuf_indexed<'src>(
    input: IndexedByteRegion<'src>,
    root: &'static RhoMatchProgram,
    family: ParComponentTag,
    claimed_score: &[u8],
) -> Result<CanonicalComponentAdmission, CanonicalProtobufAdmissionError> {
    let root_start = input
        .position_at(0)
        .expect("indexed protobuf component owns its start")
        .physical;
    validate_canonical_protobuf_inner(input, root, Some((family, claimed_score)))?
        .component()
        .ok_or(CanonicalProtobufAdmissionError::Schema {
            type_name: root.type_name,
            field: u32::from(family.byte()),
            position: root_start,
            detail: "component admission root did not produce component metadata",
        })
}

/// Prove that one raw `Par` body is an exact image of the generated canonical
/// protobuf encoder without constructing or re-encoding a rhoapi value.
///
/// EPM1 uses this narrow entry point after its generated child PDA has
/// successfully decoded the body.  Keeping admission byte-native avoids the
/// former encode-to-`Vec<u8>` comparison while preserving child-error-first
/// precedence.
pub(crate) fn validate_canonical_par_protobuf_indexed(
    input: IndexedByteRegion<'_>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    validate_canonical_protobuf_inner(input, &PAR_PROTOBUF_REGION_PROGRAM, None).map(|_| ())
}

#[cfg(test)]
#[path = "../../../tests/support/standalone_path_admission.rs"]
mod standalone_path_admission;

#[cfg(test)]
#[path = "../../../tests/support/admission_retained_history.rs"]
mod admission_retained_history;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use prost::Message;

    use super::*;
    use crate::rhoapi::connective::ConnectiveInstance;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::g_unforgeable::UnfInstance;
    use crate::rhoapi::{
        Bundle, Connective, ConnectiveBody, EMap, ESet, Expr, GPrivate, GUnforgeable, If,
        KeyValuePair, Match, MatchCase, New, Par, Receive, ReceiveBind, Send,
    };
    use crate::rust::par_bag::{with_component_key, ParBagComponent, PAR_BAG_KEY_DOMAIN};
    use crate::rust::rhoapi_ext::EPathMap;
    use crate::rust::rholang::match_schema_tables::{
        BUNDLE_RHO_MATCH_PROGRAM, CONNECTIVE_RHO_MATCH_PROGRAM, EXPR_RHO_MATCH_PROGRAM,
        GUNFORGEABLE_RHO_MATCH_PROGRAM, IF_RHO_MATCH_PROGRAM, MATCH_RHO_MATCH_PROGRAM,
        NEW_RHO_MATCH_PROGRAM, PAR_RHO_MATCH_PROGRAM, RECEIVE_RHO_MATCH_PROGRAM,
        RHO_MATCH_REGISTRY, SEND_RHO_MATCH_PROGRAM,
    };
    use crate::rust::rholang::protobuf_schema::ProtobufField;
    use crate::rust::rholang::schema_test_adapters::{
        explicit_default_mutants, schema_fixtures, GENERATED_SCHEMA_TEST_EXTERN_COUNT,
        GENERATED_SCHEMA_TEST_EXTERN_NAMES, GENERATED_SCHEMA_TEST_MESSAGE_COUNT,
        GENERATED_SCHEMA_TEST_ONEOF_ARM_COUNT, GENERATED_SCHEMA_TEST_ONEOF_COUNT,
    };
    use crate::rust::rholang::{bincode_encoder, protobuf_decoder, protobuf_encoder};

    fn gint(value: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }])
    }

    fn admitted_par_score(value: &Par) -> Vec<u8> {
        let admission = validate_canonical_protobuf(
            ProtobufRegion::from(protobuf_encoder::encode_to_vec(value)),
            &PAR_PROTOBUF_REGION_PROGRAM,
        )
        .unwrap_or_else(|error| panic!("canonical Par was rejected: {error}"));
        admission
            .score_bytes()
            .expect("raw Par admission returns its canonical score")
            .to_vec()
    }

    fn authoritative_par_score(value: &Par) -> Vec<u8> {
        let mut score = Vec::new();
        value
            .components()
            .append_par_score_key(value.connective_used(), &mut score);
        score
    }

    fn is_authoritative_component_body<T: ParBagComponent>(value: &T, body: &[u8]) -> bool {
        with_component_key(value, |key| {
            let trailer = key.len() - std::mem::size_of::<u64>();
            let score_len = usize::try_from(u64::from_be_bytes(
                key[trailer..]
                    .try_into()
                    .expect("an RPB3 key ends in one score-length word"),
            ))
            .expect("the test score length fits usize");
            let body_start = PAR_BAG_KEY_DOMAIN.len() + 1 + score_len;
            &key[body_start..trailer] == body
        })
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

    #[test]
    fn borrowed_and_owned_roots_have_identical_admission_and_error_semantics() {
        let send = Send {
            chan: Some(gint(7)),
            data: vec![gint(11), gint(13)],
            persistent: true,
            ..Send::default()
        };
        let bytes = protobuf_encoder::encode_to_vec(&send);
        let owned = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes),
            &SEND_RHO_MATCH_PROGRAM,
        );
        let borrowed = validate_canonical_protobuf_inner(
            IndexedByteRegion::from_borrowed(&bytes, 0, 0),
            &SEND_RHO_MATCH_PROGRAM,
            None,
        );
        assert_eq!(borrowed, owned);

        let malformed = [0x18, 0x81, 0x00];
        let owned_error = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&malformed),
            &SEND_RHO_MATCH_PROGRAM,
        )
        .unwrap_err();
        let borrowed_error = validate_canonical_protobuf_inner(
            IndexedByteRegion::from_borrowed(&malformed, 0, 0),
            &SEND_RHO_MATCH_PROGRAM,
            None,
        )
        .unwrap_err();
        assert_eq!(borrowed_error, owned_error);

        let shifted_error = validate_canonical_protobuf_inner(
            IndexedByteRegion::from_borrowed(&malformed, 41, 17_000),
            &SEND_RHO_MATCH_PROGRAM,
            None,
        )
        .unwrap_err();
        assert!(shifted_error.to_string().contains("source 41"));
        assert!(shifted_error.to_string().contains("17000"));
    }

    fn assert_component_body_score_state<T: ParBagComponent>(
        value: &T,
        expected_body: Option<&[u8]>,
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
            let body = &key[score_end..trailer];
            if let Some(expected_body) = expected_body {
                assert_eq!(
                    body, expected_body,
                    "the authoritative RPB3 body differs from the generated canonical fixture"
                );
            }
            let admission = validate_canonical_protobuf_inner(
                IndexedByteRegion::from_borrowed(body, 0, 0),
                program,
                Some((T::TAG, &key[score_start..score_end])),
            )
            .unwrap_or_else(|error| panic!("canonical RPB3 component was rejected: {error}"));
            assert_eq!(
                admission.score_bytes(),
                Some(&key[score_start..score_end]),
                "the byte-native reducer returned different canonical score bytes"
            );
            let component = admission
                .component()
                .expect("component admission returns component metadata");
            assert_eq!(component.family, T::TAG);
            assert_eq!(component.complete, expected_complete);
            assert_eq!(
                component.canonical_path_stable,
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

    fn assert_component_score_state<T: ParBagComponent>(
        value: &T,
        program: &'static RhoMatchProgram,
        expected_complete: bool,
    ) {
        assert_component_body_score_state(value, None, program, expected_complete);
    }

    fn assert_component_score<T: ParBagComponent>(value: &T, program: &'static RhoMatchProgram) {
        assert_component_score_state(value, program, true);
    }

    #[test]
    fn every_descriptor_message_field_and_oneof_arm_matches_the_recursive_oracle() {
        assert_eq!(GENERATED_SCHEMA_TEST_MESSAGE_COUNT, 62);
        assert_eq!(GENERATED_SCHEMA_TEST_EXTERN_COUNT, 2);
        assert_eq!(GENERATED_SCHEMA_TEST_ONEOF_COUNT, 6);
        assert_eq!(GENERATED_SCHEMA_TEST_ONEOF_ARM_COUNT, 58);
        assert_eq!(GENERATED_SCHEMA_TEST_EXTERN_NAMES, &["EPathMap", "Par"]);

        let fixtures = schema_fixtures();
        let covered_messages = fixtures
            .iter()
            .map(|fixture| fixture.message_name)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(covered_messages.len(), GENERATED_SCHEMA_TEST_MESSAGE_COUNT);

        for fixture in fixtures {
            let label = format!("{}.{}", fixture.message_name, fixture.fixture_name);
            let machine = (fixture.machine_round_trip)(&fixture.bytes)
                .unwrap_or_else(|error| panic!("{label}: generated PDA rejected fixture: {error}"));
            let oracle = (fixture.oracle_round_trip)(&fixture.bytes).unwrap_or_else(|error| {
                panic!("{label}: recursive Prost rejected fixture: {error}")
            });
            assert_eq!(machine, oracle, "{label}: PDA/oracle re-encoding differs");
            assert_eq!(
                machine, fixture.bytes,
                "{label}: fixture is not a protobuf fixed point"
            );
            let (machine_bincode, oracle_bincode) = (fixture.bincode_pair)(&fixture.bytes)
                .unwrap_or_else(|error| panic!("{label}: bincode differential: {error}"));
            assert_eq!(
                machine_bincode, oracle_bincode,
                "{label}: generated bincode encoder differs from derived Serialize"
            );

            let admission = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&fixture.bytes),
                fixture.program,
            )
            .unwrap_or_else(|error| {
                panic!("{label}: descriptor-generated canonical fixture was rejected: {error}")
            });
            let stats = admission.stats();
            assert_eq!(stats.reconstructed_key_bytes, 0, "{label}");
            assert_eq!(stats.legacy_endpoint_replays, 0, "{label}");
            assert_eq!(stats.typed_decodes, 0, "{label}");
            assert_eq!(stats.byte_native_validations, 1, "{label}");
        }
    }

    #[test]
    fn generated_rpb3_table_differential_is_bound_to_admission_runner() {
        let mut checked = 0usize;
        let mut covered_messages = std::collections::BTreeSet::new();
        for fixture in schema_fixtures() {
            covered_messages.insert(fixture.message_name);
            let (_, registered) = RHO_MATCH_REGISTRY
                .iter()
                .find(|(name, _)| *name == fixture.message_name)
                .unwrap_or_else(|| {
                    panic!("{} has no generated registry row", fixture.message_name)
                });
            assert!(
                std::ptr::eq(*registered, fixture.program),
                "{}.{} is not bound to its generated registry program",
                fixture.message_name,
                fixture.fixture_name
            );

            let machine = (fixture.machine_round_trip)(&fixture.bytes).unwrap_or_else(|error| {
                panic!(
                    "{}.{} generated PDA rejected fixture: {error}",
                    fixture.message_name, fixture.fixture_name
                )
            });
            let oracle = (fixture.oracle_round_trip)(&fixture.bytes).unwrap_or_else(|error| {
                panic!(
                    "{}.{} recursive Prost rejected fixture: {error}",
                    fixture.message_name, fixture.fixture_name
                )
            });
            assert_eq!(machine, oracle, "generated PDA/oracle disagreement");
            assert_eq!(
                machine, fixture.bytes,
                "fixture is not a protobuf fixed point"
            );

            let admission = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&fixture.bytes),
                fixture.program,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{}.{} canonical admission failed: {error}",
                    fixture.message_name, fixture.fixture_name
                )
            });
            assert_eq!(admission.stats().typed_decodes, 0);
            assert_eq!(admission.stats().legacy_endpoint_replays, 0);
            assert_eq!(admission.stats().byte_native_validations, 1);
            checked += 1;
        }
        assert!(checked > 0, "generated RPB3 fixture matrix became vacuous");
        assert_eq!(covered_messages.len(), GENERATED_SCHEMA_TEST_MESSAGE_COUNT);
        assert_eq!(
            GENERATED_SCHEMA_TEST_EXTERN_NAMES,
            &["EPathMap", "Par"],
            "external RPB3 roots must remain explicit exceptions"
        );
    }

    #[test]
    fn every_generated_rpb3_family_fixture_returns_the_authoritative_body_and_score() {
        macro_rules! check_fixture {
            ($fixture:ident, $decoder:ident) => {{
                let value =
                    protobuf_decoder::$decoder($fixture.bytes.as_slice()).unwrap_or_else(|error| {
                        panic!(
                            "{}.{}: generated decoder rejected its fixture: {error}",
                            $fixture.message_name, $fixture.fixture_name
                        )
                    });
                let complete = ParBagComponent::semantic_shape_is_complete(&value);
                assert_component_body_score_state(
                    &value,
                    Some(&$fixture.bytes),
                    $fixture.program,
                    complete,
                );
            }};
        }

        let mut family_counts = BTreeMap::new();
        for fixture in schema_fixtures() {
            match fixture.message_name {
                "Send" => check_fixture!(fixture, decode_send),
                "Receive" => check_fixture!(fixture, decode_receive),
                "New" => check_fixture!(fixture, decode_new),
                "Expr" => check_fixture!(fixture, decode_expr),
                "Match" => check_fixture!(fixture, decode_match),
                "GUnforgeable" => check_fixture!(fixture, decode_g_unforgeable),
                "Connective" => check_fixture!(fixture, decode_connective),
                "Bundle" => check_fixture!(fixture, decode_bundle),
                "If" => check_fixture!(fixture, decode_if),
                _ => continue,
            }
            *family_counts.entry(fixture.message_name).or_insert(0usize) += 1;
        }

        assert_eq!(
            family_counts.keys().copied().collect::<Vec<_>>(),
            vec![
                "Bundle",
                "Connective",
                "Expr",
                "GUnforgeable",
                "If",
                "Match",
                "New",
                "Receive",
                "Send",
            ],
            "the generated semantic differential no longer covers all nine RPB3 families"
        );
        assert!(
            family_counts.values().all(|count| *count >= 2),
            "every RPB3 family needs at least its default and one populated descriptor witness"
        );
    }

    #[test]
    fn raw_par_score_preserves_multiplicity_and_quotients_only_root_locally_free() {
        let empty = Par::default();
        let locally_free_empty = Par::default().with_locally_free(vec![1, 3, 5]);
        let once = gint(7);
        let once_with_locally_free = once.clone().with_locally_free(vec![2, 4, 6]);
        let twice = Par::default().with_exprs(vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(7)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(7)),
            },
        ]);
        let connective = once.clone().with_connective_used(true);

        for value in [
            &empty,
            &locally_free_empty,
            &once,
            &once_with_locally_free,
            &twice,
            &connective,
        ] {
            assert_eq!(
                admitted_par_score(value),
                authoritative_par_score(value),
                "byte-native raw-Par reduction differs from the PathMap<Multiplicity> authority"
            );
        }

        assert_eq!(
            admitted_par_score(&empty),
            admitted_par_score(&locally_free_empty),
            "root locally_free is a cache quotient and must not enter semantic score"
        );
        assert_eq!(
            admitted_par_score(&once),
            admitted_par_score(&once_with_locally_free),
            "root locally_free is the only ignored root byte region"
        );
        assert_ne!(
            admitted_par_score(&once),
            admitted_par_score(&twice),
            "omitting a PathMap endpoint multiplicity must turn the semantic gate red"
        );
        assert_ne!(
            admitted_par_score(&once),
            admitted_par_score(&connective),
            "root connective_used remains semantic"
        );
    }

    #[test]
    fn eset_and_emap_admission_bind_identity_representatives_and_value_winners() {
        let first_equal_key = gint(1).with_locally_free(vec![9]);
        let later_equal_key = gint(1).with_locally_free(vec![7]);
        let lesser_value = gint(2);
        let greater_value = gint(3);

        let source_set = Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: vec![first_equal_key.clone(), later_equal_key.clone()],
                ..ESet::default()
            })),
        };
        rejects(
            protobuf_encoder::encode_to_vec(&source_set),
            &EXPR_RHO_MATCH_PROGRAM,
        );
        let canonical_set = <Expr as ParBagComponent>::canonicalize(&source_set).term;
        let Some(ExprInstance::ESetBody(set)) = &canonical_set.expr_instance else {
            panic!("ESet canonicalization changed constructor family");
        };
        assert_eq!(set.ps.len(), 1);
        assert_eq!(
            set.ps[0].locally_free(),
            first_equal_key.locally_free(),
            "ESet must retain the first semantic-equality representative"
        );
        assert_component_body_score_state(
            &canonical_set,
            Some(&protobuf_encoder::encode_to_vec(&canonical_set)),
            &EXPR_RHO_MATCH_PROGRAM,
            true,
        );

        let source_map = Expr {
            expr_instance: Some(ExprInstance::EMapBody(EMap {
                kvs: vec![
                    KeyValuePair {
                        key: Some(first_equal_key.clone()),
                        value: Some(lesser_value),
                    },
                    KeyValuePair {
                        key: Some(later_equal_key),
                        value: Some(greater_value.clone()),
                    },
                ],
                ..EMap::default()
            })),
        };
        rejects(
            protobuf_encoder::encode_to_vec(&source_map),
            &EXPR_RHO_MATCH_PROGRAM,
        );
        let canonical_map = <Expr as ParBagComponent>::canonicalize(&source_map).term;
        let Some(ExprInstance::EMapBody(map)) = &canonical_map.expr_instance else {
            panic!("EMap canonicalization changed constructor family");
        };
        assert_eq!(map.kvs.len(), 1);
        let winner = &map.kvs[0];
        assert_eq!(
            winner
                .key
                .as_ref()
                .expect("canonical EMap key")
                .locally_free(),
            first_equal_key.locally_free(),
            "EMap must retain the first semantic-equality key representative"
        );
        assert_eq!(
            winner.value.as_ref(),
            Some(&greater_value),
            "EMap must retain the greatest emitted-byte value for an equal key"
        );
        assert_component_body_score_state(
            &canonical_map,
            Some(&protobuf_encoder::encode_to_vec(&canonical_map)),
            &EXPR_RHO_MATCH_PROGRAM,
            true,
        );
    }

    #[test]
    fn pathmap_backed_externs_keep_specialized_wire_and_admission_paths() {
        let set = EPathMap::from_set_iter([gint(1), gint(2)], Vec::new(), false, None);
        let map = EPathMap::new_map(
            [(gint(1), gint(11)), (gint(2), gint(22))],
            Vec::new(),
            false,
            None,
        );
        for (label, value) in [("empty", EPathMap::default()), ("set", set), ("map", map)] {
            let bytes = value.encode_to_vec();
            let oracle = EPathMap::decode(bytes.as_slice())
                .unwrap_or_else(|error| panic!("EPathMap.{label}: oracle decode: {error}"));
            let machine = protobuf_decoder::decode_epath_map(bytes.as_slice())
                .unwrap_or_else(|error| panic!("EPathMap.{label}: PDA decode: {error}"));
            assert_eq!(
                machine.encode_to_vec(),
                oracle.encode_to_vec(),
                "EPathMap.{label}"
            );
            assert_eq!(
                machine.encode_to_vec(),
                bytes,
                "EPathMap.{label}: not a fixed point"
            );
            assert_eq!(
                bincode_encoder::encode(&value),
                bincode::serialize(&value).expect("EPathMap bincode oracle"),
                "EPathMap.{label}: bincode differential"
            );
            validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&bytes),
                &EPATHMAP_RHO_MATCH_PROGRAM,
            )
            .unwrap_or_else(|error| panic!("EPathMap.{label}: admission: {error}"));
        }

        for (label, value) in [("empty", Par::default()), ("component", gint(7))] {
            let bytes = value.encode_to_vec();
            let oracle = Par::decode(bytes.as_slice())
                .unwrap_or_else(|error| panic!("Par.{label}: oracle decode: {error}"));
            let machine = protobuf_decoder::decode_par(bytes.as_slice())
                .unwrap_or_else(|error| panic!("Par.{label}: PDA decode: {error}"));
            assert_eq!(
                protobuf_encoder::encode_to_vec(&machine),
                oracle.encode_to_vec(),
                "Par.{label}"
            );
            assert_eq!(
                protobuf_encoder::encode_to_vec(&machine),
                bytes,
                "Par.{label}: not a fixed point"
            );
            assert_eq!(
                bincode_encoder::encode(&value),
                bincode::serialize(&value).expect("Par bincode oracle"),
                "Par.{label}: bincode differential"
            );
            validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&bytes),
                &PAR_PROTOBUF_REGION_PROGRAM,
            )
            .unwrap_or_else(|error| panic!("Par.{label}: admission: {error}"));
        }
    }

    #[test]
    fn descriptor_wide_hostile_wires_match_decode_plus_canonical_reencode() {
        let fixtures = schema_fixtures();
        let mut comparisons = 0usize;
        let mut covered_fixtures = 0usize;
        for fixture in fixtures {
            covered_fixtures += 1;
            let mut candidates = Vec::new();
            for cut in 0..fixture.bytes.len() {
                candidates.push(("truncation", fixture.bytes[..cut].to_vec()));
            }

            let mut unknown = fixture.bytes.clone();
            unknown.extend_from_slice(&[0xf8, 0x07, 0x01]);
            candidates.push(("unknown_field", unknown));

            if !fixture.bytes.is_empty() {
                let mut duplicate = fixture.bytes.clone();
                duplicate.extend_from_slice(&fixture.bytes);
                candidates.push(("duplicate_image", duplicate));

                if fixture.bytes[0] < 0x80 {
                    let mut nonminimal_key = Vec::with_capacity(fixture.bytes.len() + 1);
                    nonminimal_key.push(fixture.bytes[0] | 0x80);
                    nonminimal_key.push(0);
                    nonminimal_key.extend_from_slice(&fixture.bytes[1..]);
                    candidates.push(("nonminimal_key", nonminimal_key));
                }
            }

            for (mutation, bytes) in candidates {
                let machine_fixed_point = (fixture.machine_round_trip)(&bytes)
                    .map(|canonical| canonical == bytes)
                    .unwrap_or(false);
                let oracle_fixed_point = (fixture.oracle_round_trip)(&bytes)
                    .map(|canonical| canonical == bytes)
                    .unwrap_or(false);
                let label = format!(
                    "{}.{}::{mutation}",
                    fixture.message_name, fixture.fixture_name
                );
                assert_eq!(
                    machine_fixed_point, oracle_fixed_point,
                    "{label}: PDA and Prost disagree on strict fixed-point admission"
                );
                macro_rules! component_fixed_point {
                    ($decoder:ident) => {
                        protobuf_decoder::$decoder(bytes.as_slice())
                            .map(|value| is_authoritative_component_body(&value, &bytes))
                            .unwrap_or(false)
                    };
                }
                let semantic_fixed_point = match fixture.message_name {
                    "Send" => component_fixed_point!(decode_send),
                    "Receive" => component_fixed_point!(decode_receive),
                    "New" => component_fixed_point!(decode_new),
                    "Expr" => component_fixed_point!(decode_expr),
                    "Match" => component_fixed_point!(decode_match),
                    "GUnforgeable" => component_fixed_point!(decode_g_unforgeable),
                    "Connective" => component_fixed_point!(decode_connective),
                    "Bundle" => component_fixed_point!(decode_bundle),
                    "If" => component_fixed_point!(decode_if),
                    _ => true,
                };
                let strict_fixed_point = oracle_fixed_point && semantic_fixed_point;
                let admitted = validate_canonical_protobuf(
                    ProtobufRegion::copy_from_slice(&bytes),
                    fixture.program,
                )
                .is_ok();
                assert_eq!(
                    admitted, strict_fixed_point,
                    "{label}: byte-native admission differs from decode, semantic \
                     canonicalization, and exact re-encoding"
                );
                comparisons += 1;
            }
        }
        assert!(
            covered_fixtures > 0 && comparisons >= covered_fixtures * 4,
            "hostile schema matrix became vacuous: {covered_fixtures} fixtures, \
             {comparisons} comparisons"
        );
    }

    #[test]
    fn every_descriptor_scalar_field_rejects_explicit_default_presence() {
        let mutants = explicit_default_mutants();
        assert!(
            mutants.len() >= 32,
            "explicit-default matrix became vacuous: only {} scalar fields",
            mutants.len()
        );
        for mutant in mutants {
            let label = format!("{}.{}", mutant.message_name, mutant.fixture_name);
            assert!(!mutant.bytes.is_empty(), "{label}: mutation did not apply");
            let machine = (mutant.machine_round_trip)(&mutant.bytes).unwrap_or_else(|error| {
                panic!("{label}: PDA did not accept protobuf alias: {error}")
            });
            let oracle = (mutant.oracle_round_trip)(&mutant.bytes).unwrap_or_else(|error| {
                panic!("{label}: Prost did not accept protobuf alias: {error}")
            });
            assert_eq!(machine, oracle, "{label}: alias re-encoding differs");
            assert_ne!(
                machine, mutant.bytes,
                "{label}: explicit default stayed canonical"
            );
            assert!(
                validate_canonical_protobuf(
                    ProtobufRegion::copy_from_slice(&mutant.bytes),
                    mutant.program,
                )
                .is_err(),
                "{label}: byte-native admission accepted explicit scalar default"
            );
        }
    }

    #[test]
    fn every_forbidden_cutover_fallback_makes_the_verdict_red() {
        let baseline = CanonicalProtobufAdmissionStats {
            byte_native_validations: 1,
            ..CanonicalProtobufAdmissionStats::default()
        };
        validate_cutover_stats(&baseline).expect("the byte-native control must remain green");

        let mut reconstructed = baseline;
        reconstructed.reconstructed_key_bytes = 1;
        let mut replayed = baseline;
        replayed.legacy_endpoint_replays = 1;
        let mut typed = baseline;
        typed.typed_decodes = 1;
        let mut skipped = baseline;
        skipped.byte_native_validations = 0;
        let mut repeated = baseline;
        repeated.byte_native_validations = 2;

        for (name, mutant) in [
            ("reconstructed_key", reconstructed),
            ("endpoint_replay", replayed),
            ("typed_decode", typed),
            ("skipped_validation", skipped),
            ("repeated_validation", repeated),
        ] {
            assert_ne!(mutant, baseline, "{name}: mutation did not apply");
            assert!(
                matches!(
                    validate_cutover_stats(&mutant),
                    Err(CanonicalProtobufAdmissionError::CutoverInvariant(_))
                ),
                "{name}: the production cutover verdict admitted a forbidden fallback"
            );
        }
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
            cost_region: None,
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

        // The complete hostile scalar/framing classes remain independently
        // observable rather than relying on a single overlong field key.
        for (bytes, program) in [
            (vec![0x00], &SEND_RHO_MATCH_PROGRAM), // forbidden field tag zero
            (vec![0x18, 0x02], &SEND_RHO_MATCH_PROGRAM), // noncanonical true alias
            (vec![0x1a, 0x00], &SEND_RHO_MATCH_PROGRAM), // bool with wrong wire type
            (vec![0x18, 0x01, 0x18, 0x01], &SEND_RHO_MATCH_PROGRAM), // duplicate singular field
            (vec![0x0b, 0x0c], &SEND_RHO_MATCH_PROGRAM), // unsupported group framing
            (vec![0x1a, 0x81, 0x00, b'a'], &EXPR_RHO_MATCH_PROGRAM), // nonminimal string length
            (vec![0x1a, 0xff, 0xff, 0xff, 0xff], &EXPR_RHO_MATCH_PROGRAM), // truncated oversized length
        ] {
            rejects(bytes, program);
        }
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
    fn every_epm1_framing_layer_and_homogeneous_mode_is_admitted_once() {
        fn snapshot_from_root(bytes: &[u8]) -> Vec<u8> {
            let start = bytes
                .windows(4)
                .position(|window| window == b"EPM1")
                .expect("canonical EPathMap protobuf contains its EPM1 snapshot");
            bytes[start..].to_vec()
        }

        fn wrap_snapshot(snapshot: &[u8]) -> Vec<u8> {
            let mut bytes = vec![0x4a]; // EPathMap.trie_snapshot = field 9.
            prost::encoding::encode_varint(snapshot.len() as u64, &mut bytes);
            bytes.extend_from_slice(snapshot);
            bytes
        }

        fn read_varint(bytes: &[u8], cursor: &mut usize) -> usize {
            let mut value = 0usize;
            let mut shift = 0usize;
            loop {
                let byte = bytes[*cursor];
                *cursor += 1;
                value |= usize::from(byte & 0x7f) << shift;
                if byte & 0x80 == 0 {
                    return value;
                }
                shift += 7;
            }
        }

        let set = EPathMap::from_set_iter([gint(1), gint(2)], Vec::new(), false, None);
        let map = EPathMap::new_map(
            [(gint(1), gint(11)), (gint(2), gint(22))],
            Vec::new(),
            false,
            None,
        );

        for (label, value) in [("empty", EPathMap::default()), ("set", set), ("map", map)] {
            let root = value.encode_to_vec();
            let admission = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&root),
                &EPATHMAP_RHO_MATCH_PROGRAM,
            )
            .unwrap_or_else(|error| panic!("{label}: canonical EPM1 rejected: {error}"));
            assert_eq!(admission.stats().epathmaps, 1, "{label}");

            let snapshot = snapshot_from_root(&root);
            assert_eq!(&snapshot[..4], b"EPM1", "{label}");
            assert_eq!(snapshot[4], 1, "{label}: EPM1 version");

            let mut mutations = Vec::new();
            let mut bad_magic = snapshot.clone();
            bad_magic[0] ^= 1;
            mutations.push(("magic", bad_magic));
            let mut bad_version = snapshot.clone();
            bad_version[4] = 2;
            mutations.push(("version", bad_version));
            let mut bad_mode = snapshot.clone();
            bad_mode[5] = 0xff;
            mutations.push(("unknown_mode", bad_mode));
            let mut mixed_mode = snapshot.clone();
            mixed_mode[5] = match snapshot[5] {
                0 => 1,
                1 => 2,
                2 => 1,
                _ => unreachable!("canonical EPM1 mode"),
            };
            mutations.push(("mixed_mode", mixed_mode));
            let mut trailing = snapshot.clone();
            trailing.push(0);
            mutations.push(("trailing_byte", trailing));

            let mut cursor = 6usize;
            let arena_len = read_varint(&snapshot, &mut cursor);
            if arena_len != 0 {
                let mut bad_act = snapshot.clone();
                bad_act[cursor] ^= 1;
                mutations.push(("act_magic", bad_act));

                let value_count_at = cursor + arena_len;
                let mut bad_count = snapshot.clone();
                bad_count[value_count_at] = if snapshot[5] == 1 { 1 } else { 0 };
                mutations.push(("value_count", bad_count));
            }

            for (mutation, snapshot) in mutations {
                assert!(
                    validate_canonical_protobuf(
                        ProtobufRegion::from(wrap_snapshot(&snapshot)),
                        &EPATHMAP_RHO_MATCH_PROGRAM,
                    )
                    .is_err(),
                    "{label}.{mutation}: malformed EPM1 was admitted"
                );
            }

            let mut mixed_legacy = root.clone();
            mixed_legacy.extend_from_slice(&[0x0a, 0x00]);
            rejects(mixed_legacy, &EPATHMAP_RHO_MATCH_PROGRAM);
        }
    }

    fn test_varint_len(mut value: usize) -> usize {
        let mut len = 1usize;
        while value >= 0x80 {
            value >>= 7;
            len += 1;
        }
        len
    }

    fn push_test_varint(out: &mut Vec<u8>, mut value: usize) {
        while value >= 0x80 {
            out.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    /// Iteratively encode `Par → Expr::ENot → Par` without retaining all AST
    /// suffixes. The size pass is bottom-up and the write pass root-down, so
    /// construction is linear in the final bytes and uses no native recursion.
    fn nested_not_protobuf(depth: usize) -> Vec<u8> {
        let mut lengths = vec![(0usize, 0usize, 0usize); depth];
        let mut inner_par_len = 0usize;
        for slot in lengths.iter_mut().rev() {
            let enot_len = 1 + test_varint_len(inner_par_len) + inner_par_len;
            let expr_len = 1 + test_varint_len(enot_len) + enot_len;
            let par_len = 1 + test_varint_len(expr_len) + expr_len;
            *slot = (par_len, expr_len, enot_len);
            inner_par_len = par_len;
        }
        let mut out = Vec::with_capacity(inner_par_len);
        for (level, &(_, expr_len, enot_len)) in lengths.iter().enumerate() {
            let child_par_len = lengths.get(level + 1).map_or(0, |entry| entry.0);
            out.push(0x2a);
            push_test_varint(&mut out, expr_len);
            out.push(0x2a);
            push_test_varint(&mut out, enot_len);
            out.push(0x0a);
            push_test_varint(&mut out, child_par_len);
        }
        assert_eq!(out.len(), inner_par_len);
        out
    }

    #[test]
    fn depth_20000_validation_uses_the_heap_worklist_on_a_small_native_stack() {
        let bytes = nested_not_protobuf(20_000);
        std::thread::Builder::new()
            .name("canonical-protobuf-admission-depth-20000".to_string())
            .stack_size(256 * 1024)
            .spawn(move || {
                let admission = validate(bytes, &PAR_PROTOBUF_REGION_PROGRAM);
                assert!(admission.stats().max_frames >= 20_000);
            })
            .expect("spawn small-stack canonical protobuf validator")
            .join()
            .expect("canonical protobuf validator completes");
    }

    #[test]
    fn width_4096_validation_uses_counted_occurrences_on_a_small_native_stack() {
        std::thread::Builder::new()
            .name("canonical-protobuf-admission-width-4096".to_string())
            .stack_size(256 * 1024)
            .spawn(|| {
                let exprs = (0..4_096)
                    .map(|value| Expr {
                        expr_instance: Some(ExprInstance::GInt(value)),
                    })
                    .collect();
                let bytes = protobuf_encoder::encode_to_vec(&Par::default().with_exprs(exprs));
                let admission = validate(bytes, &PAR_PROTOBUF_REGION_PROGRAM);
                assert!(admission.stats().fields >= 4_096);
            })
            .expect("spawn small-stack wide canonical protobuf validator")
            .join()
            .expect("wide canonical protobuf validator completes");
    }
}
