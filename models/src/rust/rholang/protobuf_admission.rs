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
use verified_score_store::admission_event_checker::{
    MonitorRefusal, ObservedAcceptRecord, ObservedEvent, ObservedField, ObservedFrameRequest,
    ObservedSnapshot,
};
use verified_score_store::{SlotAdmissionError, VerifiedMessageSlots};

use super::canonical_bytes_arena::CanonicalByteProvenance;
#[cfg(test)]
use super::canonical_bytes_arena::{CanonicalBytesArena, CanonicalLocatedByte};
#[cfg(test)]
use super::canonical_path_admission::CanonicalPathParserSourceImage;
use super::canonical_path_admission::{
    CanonicalPathBatchHandleSource, CanonicalPathCompletedObligation, CanonicalPathObligation,
    CanonicalPathObligationId, CanonicalPathObligationKind, CanonicalPathObligationProgress,
    CanonicalPathObligationResolution, CanonicalPathSource, CanonicalPathStreamReducer,
    CanonicalPathSummary, CanonicalPathTopologyAdmission, CanonicalPathTransition, LocatedPathByte,
    NestedEpmMode,
};
use super::canonical_score_bytes::{
    CanonicalScoreAppendCheckpoint, CanonicalScoreArena, CanonicalScoreArenaError,
    CanonicalScoreBytesView, CanonicalScoreHandle, CanonicalScoreNodeView,
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
    IndexedCanonicalFieldCursorStats, IndexedCanonicalFieldValue, IndexedLocatedByteCursor,
    IndexedMessagePreReadSource, LocatedCanonicalProtobufError,
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
    EPathMapMode, IndexedCanonicalEpm1TerminalSourceImage, IndexedCanonicalEpm1TopologyCursor,
    IndexedCanonicalEpm1TopologyEvent, IndexedCanonicalEpm1TopologySourceEvent,
    IndexedCanonicalEpm1TopologyStats, TrieCodecError,
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
    VerifiedMonitorRefused {
        step: u64,
        refusal: MonitorRefusal,
    },
    VerifiedMonitorIncomplete {
        checked_events: u64,
    },
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
            Self::VerifiedMonitorRefused { step, refusal } => write!(
                formatter,
                "verified admission monitor refused checked event {step}: {refusal:?}"
            ),
            Self::VerifiedMonitorIncomplete { checked_events } => write!(
                formatter,
                "verified admission monitor checked {checked_events} events without accepting every driver iteration"
            ),
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
            | Self::SizeOverflow
            | Self::VerifiedMonitorRefused { .. }
            | Self::VerifiedMonitorIncomplete { .. } => None,
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

impl SemanticScalar<'_> {
    fn same_source_value(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(left), Self::Bool(right)) => left == right,
            (Self::I32(left), Self::I32(right)) => left == right,
            (Self::U32(left), Self::U32(right)) => left == right,
            (Self::I64(left), Self::I64(right)) => left == right,
            (Self::U64(left), Self::U64(right)) => left == right,
            (Self::String(left), Self::String(right)) | (Self::Bytes(left), Self::Bytes(right)) => {
                left.same_view(right)
            }
            _ => false,
        }
    }
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

impl SemanticSummary<'_> {
    fn matches_kind(&self, kind: SemanticKind) -> bool {
        match kind {
            SemanticKind::Opaque => matches!(self, Self::Opaque),
            SemanticKind::RawPar => matches!(self, Self::Par(_)),
            SemanticKind::Component(family) => {
                matches!(self, Self::Component(component) if component.family == family)
            }
            SemanticKind::Var => matches!(self, Self::Var(_)),
            SemanticKind::Unit => matches!(self, Self::Unit),
            SemanticKind::ReceiveBind => matches!(self, Self::ReceiveBind { .. }),
            SemanticKind::CostSignature
            | SemanticKind::CostSignatureAtom
            | SemanticKind::CostBoundSignature
            | SemanticKind::CostRegion => matches!(self, Self::CostScore(_)),
            SemanticKind::MatchCase => matches!(self, Self::MatchCase { .. }),
            SemanticKind::Expr => {
                matches!(self, Self::Component(component) if component.family == ParComponentTag::Expr)
            }
            SemanticKind::Connective => {
                matches!(self, Self::Component(component) if component.family == ParComponentTag::Connective)
            }
            SemanticKind::GUnforgeable => {
                matches!(self, Self::Component(component) if component.family == ParComponentTag::GUnforgeable)
            }
            SemanticKind::BigRational => matches!(self, Self::BigRational { .. }),
            SemanticKind::FixedPoint => matches!(self, Self::FixedPoint { .. }),
            SemanticKind::OrderedCollection { .. }
            | SemanticKind::EMap
            | SemanticKind::EPathMap
            | SemanticKind::EZipper
            | SemanticKind::EMethod
            | SemanticKind::ExprVar
            | SemanticKind::ExprUnary(_)
            | SemanticKind::ExprBinary(_) => matches!(self, Self::ExprPayload(_)),
            SemanticKind::KeyValuePair => matches!(self, Self::KeyValuePair { .. }),
            SemanticKind::VarRef => matches!(self, Self::VarRef { .. }),
            SemanticKind::ConnectiveBody => matches!(self, Self::ConnectiveBody(_)),
            SemanticKind::BytesLeaf => matches!(self, Self::Bytes(_)),
        }
    }
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
    slots: VerifiedMessageSlots,
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
            slots: VerifiedMessageSlots::new(slots),
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

    fn append_semantic_occurrence(&mut self, slot: usize, occurrence: SemanticOccurrence<'src>) {
        let _seen = &self.slots;
        let (before, selected_and_after) = self.semantic_slots.split_at_mut(slot);
        let (selected, after) = selected_and_after
            .split_first_mut()
            .expect("a generated field names a semantic slot");
        let _before: &[Vec<SemanticOccurrence<'src>>] = before;
        let _after: &[Vec<SemanticOccurrence<'src>>] = after;
        selected.push(occurrence);
        let _ = (_seen, _before, _after);
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
                self.append_semantic_occurrence(slot, SemanticOccurrence {
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
                self.append_semantic_occurrence(slot, SemanticOccurrence {
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
                #[cfg(test)]
                let returned_snapshot = summary.snapshot.clone();
                self.append_semantic_occurrence(slot, SemanticOccurrence {
                    variant: None,
                    body: Some(summary.snapshot.clone()),
                    value: SemanticValue::EPathMap(Box::new(summary)),
                    field_position,
                });
                #[cfg(test)]
                rpb3_root_finish_trace::record_epathmap_accept(
                    self,
                    slot,
                    &returned_snapshot,
                    self.semantic_slots[slot]
                        .last()
                        .and_then(|occurrence| occurrence.body.as_ref())
                        .expect("accepted EPathMap occurrence retains its snapshot"),
                );
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
        &self,
        scores: &mut CanonicalScoreArena<'src>,
    ) -> Result<MessageFinishObservation<'src>, CanonicalProtobufAdmissionError> {
        if self.waiting.is_some() {
            return Err(self.schema_error(0, "protobuf message completed with a pending child"));
        }
        for (slot, field) in self.program.fields.iter().enumerate() {
            if matches!(field.kind, RhoMatchFieldKind::PathMap)
                && self.slots.seen_at(slot) != Some(true)
            {
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
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<MessageFinishObservation<'src>, CanonicalProtobufAdmissionError> {
    let kind = semantic_kind(frame.program)
        .ok_or_else(|| frame.schema_error(0, "message has no canonical semantic reducer"))?;
    let semantic = reduce_semantic_kind(frame, scores, kind)?;
    #[cfg(test)]
    rpb3_root_finish_trace::record_reduced(frame, &semantic, scores);
    Ok(MessageFinishObservation {
        selected_kind: kind,
        summary: MessageSummary { semantic },
    })
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
        SemanticKind::CostBoundSignature => reduce_cost_bound_signature(frame, scores),
        SemanticKind::CostRegion => reduce_cost_region(frame, scores),
        SemanticKind::MatchCase => reduce_match_case(frame, scores),
        SemanticKind::Expr => reduce_expr(frame, scores),
        SemanticKind::BigRational => reduce_big_rational(frame),
        SemanticKind::FixedPoint => reduce_fixed_point(frame),
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
        SemanticKind::VarRef => reduce_var_ref(frame),
        SemanticKind::ConnectiveBody => reduce_connective_body(frame),
        SemanticKind::GUnforgeable => reduce_unforgeable(frame, scores),
        SemanticKind::BytesLeaf => reduce_bytes_leaf(frame),
    }
}

// Keep variant-specific temporaries out of the common message reducer's
// debug stack frame. Component EOFs use the same dispatch but need none of
// these byte-region and collection construction locals.
#[cfg_attr(debug_assertions, inline(never))]
fn reduce_cost_bound_signature<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    Ok(SemanticSummary::CostScore(semantics::cost_bound_score(
        scores,
        i32_value(frame, "index")?,
    )?))
}

#[cfg_attr(debug_assertions, inline(never))]
fn reduce_big_rational<'src>(
    frame: &MessageFrame<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let numerator = bytes_value(frame, "numerator")?.unwrap_or_else(|| empty_region(&frame.body));
    let denominator =
        bytes_value(frame, "denominator")?.unwrap_or_else(|| empty_region(&frame.body));
    Ok(SemanticSummary::BigRational {
        numerator,
        denominator,
    })
}

#[cfg_attr(debug_assertions, inline(never))]
fn reduce_fixed_point<'src>(
    frame: &MessageFrame<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    Ok(SemanticSummary::FixedPoint {
        unscaled: bytes_value(frame, "unscaled")?.unwrap_or_else(|| empty_region(&frame.body)),
        scale: u32_value(frame, "scale")?,
    })
}

#[cfg_attr(debug_assertions, inline(never))]
fn reduce_var_ref<'src>(
    frame: &MessageFrame<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    Ok(SemanticSummary::VarRef {
        index: i32_value(frame, "index")?,
        depth: i32_value(frame, "depth")?,
    })
}

#[cfg_attr(debug_assertions, inline(never))]
fn reduce_connective_body<'src>(
    frame: &MessageFrame<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let mut pars = Vec::new();
    for occurrence in semantic_slot(frame, "ps")? {
        pars.push(par_occurrence(occurrence, frame)?.0.clone());
    }
    Ok(SemanticSummary::ConnectiveBody(pars))
}

#[cfg_attr(debug_assertions, inline(never))]
fn reduce_bytes_leaf<'src>(
    frame: &MessageFrame<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    let mut value = None;
    for field in frame.program.fields {
        if let Some(bytes) = bytes_value(frame, field.name)? {
            value = Some(bytes);
            break;
        }
    }
    Ok(SemanticSummary::Bytes(
        value.unwrap_or_else(|| empty_region(&frame.body)),
    ))
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

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReceiveBindFinishInput {
    SourceAbsent,
    SourcePresent {
        source: CanonicalScoreHandle,
        patterns: Vec<CanonicalScoreHandle>,
        remainder: Option<CanonicalScoreHandle>,
        cost: Option<CanonicalScoreHandle>,
    },
}

fn reduce_receive_bind<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    #[cfg(test)]
    let source_len_before = scores.source_len();
    let source = optional_par(frame, "source")?;
    let source_present = source.is_some();
    #[cfg(test)]
    let input;
    let score = if let Some((source, _)) = source {
        let mut patterns = Vec::new();
        for occurrence in semantic_slot(frame, "patterns")? {
            patterns.push(par_occurrence(occurrence, frame)?.0.score);
        }
        let remainder = optional_var(frame, "remainder")?;
        let absent_remainder = scores.integer(i64::from(Score::ABSENT))?;
        let selected_remainder = remainder.unwrap_or(absent_remainder);
        let cost = cost_score(frame, "cost_region")?;
        #[cfg(test)]
        {
            input = ReceiveBindFinishInput::SourcePresent {
                source: source.score,
                patterns: patterns.clone(),
                remainder,
                cost,
            };
        }
        Some(semantics::receive_bind_score(
            scores,
            source.score,
            patterns,
            selected_remainder,
            cost,
        )?)
    } else {
        #[cfg(test)]
        {
            input = ReceiveBindFinishInput::SourceAbsent;
        }
        None
    };
    #[cfg(test)]
    rpb3_root_finish_trace::record_receive_bind_finish(
        frame,
        input,
        score,
        source_len_before,
        scores.source_len(),
    );
    Ok(SemanticSummary::ReceiveBind {
        score,
        source_present,
    })
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum MatchCaseFinishInput {
    RequiredAbsent,
    RequiredPresent {
        pattern: CanonicalScoreHandle,
        source: CanonicalScoreHandle,
        guard: Option<CanonicalScoreHandle>,
        free_count: i32,
    },
}

fn reduce_match_case<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<SemanticSummary<'src>, CanonicalProtobufAdmissionError> {
    #[cfg(test)]
    let source_len_before = scores.source_len();
    let pattern = optional_par(frame, "pattern")?;
    let source = optional_par(frame, "source")?;
    let guard = optional_par(frame, "guard")?;
    let required_present = pattern.is_some() && source.is_some();
    let canonical_fixed_point = !guard.is_some_and(|(par, _)| par.semantic_default);
    #[cfg(test)]
    let input;
    let score = match (pattern, source) {
        (Some((pattern, _)), Some((source, _))) => {
            let guard_score = guard
                .map(|(guard, _)| guard.score)
                .unwrap_or(empty_par_score(scores)?);
            let free_count = i32_value(frame, "free_count")?;
            #[cfg(test)]
            {
                input = MatchCaseFinishInput::RequiredPresent {
                    pattern: pattern.score,
                    source: source.score,
                    guard: guard.map(|(guard, _)| guard.score),
                    free_count,
                };
            }
            Some(semantics::match_case_score(
                scores,
                pattern.score,
                source.score,
                free_count,
                guard_score,
            )?)
        }
        _ => {
            #[cfg(test)]
            {
                input = MatchCaseFinishInput::RequiredAbsent;
            }
            None
        }
    };
    #[cfg(test)]
    rpb3_root_finish_trace::record_match_case_finish(
        frame,
        input,
        score,
        required_present,
        canonical_fixed_point,
        source_len_before,
        scores.source_len(),
    );
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
    #[cfg(test)]
    let source_len_before = scores.source_len();
    let score = optional_var(frame, "v")?
        .map(|var| semantics::unary_score(scores, Score::EVAR, var))
        .transpose()?;
    #[cfg(test)]
    rpb3_root_finish_trace::record_expr_payload_finish(
        frame,
        score,
        source_len_before,
        scores.source_len(),
    );
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
    #[cfg(test)]
    let source_len_before = scores.source_len();
    let score = optional_par(frame, "p")?
        .map(|(par, _)| semantics::unary_score(scores, constructor, par.score))
        .transpose()?;
    #[cfg(test)]
    rpb3_root_finish_trace::record_expr_payload_finish(
        frame,
        score,
        source_len_before,
        scores.source_len(),
    );
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
    #[cfg(test)]
    let source_len_before = scores.source_len();
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
    #[cfg(test)]
    rpb3_root_finish_trace::record_expr_payload_finish(
        frame,
        score,
        source_len_before,
        scores.source_len(),
    );
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

#[cfg_attr(test, derive(Clone, Debug, PartialEq, Eq))]
enum SendFinishInput {
    Incomplete,
    Complete {
        persistent: bool,
        channel: CanonicalScoreHandle,
        data: Vec<CanonicalScoreHandle>,
        connective_used: bool,
        cost: Option<CanonicalScoreHandle>,
    },
}

fn send_finish_input<'src>(
    frame: &MessageFrame<'src>,
) -> Result<SendFinishInput, CanonicalProtobufAdmissionError> {
    let Some((channel, _)) = optional_par(frame, "chan")? else {
        return Ok(SendFinishInput::Incomplete);
    };
    let mut data = Vec::new();
    for occurrence in semantic_slot(frame, "data")? {
        data.push(par_occurrence(occurrence, frame)?.0.score);
    }
    Ok(SendFinishInput::Complete {
        persistent: bool_value(frame, "persistent")?,
        channel: channel.score,
        data,
        connective_used: bool_value(frame, "connective_used")?,
        cost: cost_score(frame, "cost_region")?,
    })
}

fn reduce_component_send<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    let input = send_finish_input(frame)?;
    #[cfg(test)]
    let source_input = input.clone();
    #[cfg(test)]
    let source_len_before = scores.source_len();
    let result = match input {
        SendFinishInput::Incomplete => (false, nonsemantic_score(scores)?),
        SendFinishInput::Complete {
            persistent,
            channel,
            data,
            connective_used,
            cost,
        } => (
            true,
            semantics::send_score(scores, persistent, channel, data, connective_used, cost)?,
        ),
    };
    #[cfg(test)]
    rpb3_root_finish_trace::record_send_finish(
        frame,
        source_input,
        result,
        source_len_before,
        scores.source_len(),
    );
    Ok(result)
}

/// Check the actual Send reducer append against the pre-EOF semantic slots.
/// This constructs no score and reads no reducer-produced `SendFinishInput`:
/// the borrowed arena source must contain the exact scalar values and child
/// handles selected by the admitted message fields.
fn check_send_append<'src>(
    frame: &MessageFrame<'src>,
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
) -> Result<(bool, usize), CanonicalProtobufAdmissionError> {
    let mismatch = || {
        CanonicalProtobufAdmissionError::CutoverInvariant(
            "Send EOF score append disagrees with pre-EOF semantic slots",
        )
    };
    let integer_at = |index: usize, expected: i64| {
        scores.source_entry_at(index).is_some_and(|entry| {
            matches!(entry.kind, CanonicalScoreNodeView::Integer(value) if value == expected)
        })
    };
    if let Some((channel, _)) = optional_par(frame, "chan")? {
        let persistent = bool_value(frame, "persistent")?;
        let connective_used = bool_value(frame, "connective_used")?;
        let data = semantic_slot(frame, "data")?;
        let cost = cost_score(frame, "cost_region")?;
        let persistent_index = before_len;
        let connective_index = before_len.checked_add(1).ok_or_else(mismatch)?;
        let constructor_index = before_len.checked_add(2).ok_or_else(mismatch)?;
        let node_index = before_len.checked_add(3).ok_or_else(mismatch)?;
        let after_len = before_len.checked_add(4).ok_or_else(mismatch)?;
        let children_len = data
            .len()
            .checked_add(4)
            .and_then(|count| count.checked_add(usize::from(cost.is_some())))
            .ok_or_else(mismatch)?;
        let Some(node) = scores.source_entry_at(node_index) else {
            return Err(mismatch());
        };
        let CanonicalScoreNodeView::Node(children) = node.kind else {
            return Err(mismatch());
        };
        if scores.source_len() != after_len
            || !integer_at(persistent_index, i64::from(persistent))
            || !integer_at(connective_index, i64::from(connective_used))
            || !integer_at(constructor_index, i64::from(Score::SEND))
            || children.len() != children_len
            || children[0].index() != constructor_index
            || children[1].index() != persistent_index
            || children[2] != channel.score
        {
            return Err(mismatch());
        }
        for (offset, occurrence) in data.iter().enumerate() {
            if children[offset + 3] != par_occurrence(occurrence, frame)?.0.score {
                return Err(mismatch());
            }
        }
        if children[data.len() + 3].index() != connective_index
            || cost.is_some_and(|handle| children[data.len() + 4] != handle)
        {
            return Err(mismatch());
        }
        Ok((true, node_index))
    } else {
        let constructor_index = before_len;
        let node_index = before_len.checked_add(1).ok_or_else(mismatch)?;
        let after_len = before_len.checked_add(2).ok_or_else(mismatch)?;
        let Some(node) = scores.source_entry_at(node_index) else {
            return Err(mismatch());
        };
        let CanonicalScoreNodeView::Node(children) = node.kind else {
            return Err(mismatch());
        };
        if scores.source_len() != after_len
            || !integer_at(
                constructor_index,
                i64::from(semantics::NONSEMANTIC_SCORE_CONSTRUCTOR),
            )
            || children.len() != 1
            || children[0].index() != constructor_index
        {
            return Err(mismatch());
        }
        Ok((false, node_index))
    }
}

/// Check the scalar prefix, constructor, and exact borrowed child handles of
/// a component append. `scalars` ends in the constructor tag; the preceding
/// values are scalar fields selected from the pre-EOF message frame.
fn check_component_constructor_append<'src>(
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
    scalars: &[i64],
    children: &[usize],
) -> Result<usize, CanonicalProtobufAdmissionError> {
    let mismatch = || {
        CanonicalProtobufAdmissionError::CutoverInvariant(
            "component EOF score append disagrees with pre-EOF semantic slots",
        )
    };
    let constructor_offset = scalars.len().checked_sub(1).ok_or_else(mismatch)?;
    let constructor_index = before_len
        .checked_add(constructor_offset)
        .ok_or_else(mismatch)?;
    let node_index = before_len.checked_add(scalars.len()).ok_or_else(mismatch)?;
    let after_len = node_index.checked_add(1).ok_or_else(mismatch)?;
    let node_children = children.len().checked_add(1).ok_or_else(mismatch)?;
    if scores.source_len() != after_len {
        return Err(mismatch());
    }
    for (offset, expected) in scalars.iter().enumerate() {
        let index = before_len.checked_add(offset).ok_or_else(mismatch)?;
        if !scores.source_entry_at(index).is_some_and(|entry| {
            matches!(entry.kind, CanonicalScoreNodeView::Integer(actual) if actual == *expected)
        }) {
            return Err(mismatch());
        }
    }
    let Some(node) = scores.source_entry_at(node_index) else {
        return Err(mismatch());
    };
    let CanonicalScoreNodeView::Node(actual_children) = node.kind else {
        return Err(mismatch());
    };
    if actual_children.len() != node_children
        || actual_children[0].index() != constructor_index
        || !actual_children[1..]
            .iter()
            .zip(children)
            .all(|(actual, expected)| actual.index() == *expected)
    {
        return Err(mismatch());
    }
    Ok(node_index)
}

fn check_nonsemantic_component_append<'src>(
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
) -> Result<usize, CanonicalProtobufAdmissionError> {
    check_component_constructor_append(
        scores,
        before_len,
        &[i64::from(semantics::NONSEMANTIC_SCORE_CONSTRUCTOR)],
        &[],
    )
}

/// Derive Bundle's exact append from the pre-EOF body and flag slots.
fn check_bundle_append<'src>(
    frame: &MessageFrame<'src>,
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
) -> Result<(bool, usize), CanonicalProtobufAdmissionError> {
    if let Some((body, _)) = optional_par(frame, "body")? {
        let constructor = semantics::bundle_constructor(
            bool_value(frame, "read_flag")?,
            bool_value(frame, "write_flag")?,
        );
        Ok((
            true,
            check_component_constructor_append(scores, before_len, &[i64::from(constructor)], &[
                body.score.index(),
            ])?,
        ))
    } else {
        Ok((
            false,
            check_nonsemantic_component_append(scores, before_len)?,
        ))
    }
}

/// Derive If's exact append from the pre-EOF condition and branch slots.
fn check_if_append<'src>(
    frame: &MessageFrame<'src>,
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
) -> Result<(bool, usize), CanonicalProtobufAdmissionError> {
    let condition = optional_par(frame, "condition")?;
    let if_true = optional_par(frame, "if_true")?;
    let if_false = optional_par(frame, "if_false")?;
    if let (Some((condition, _)), Some((if_true, _)), Some((if_false, _))) =
        (condition, if_true, if_false)
    {
        Ok((
            true,
            check_component_constructor_append(
                scores,
                before_len,
                &[
                    i64::from(bool_value(frame, "connective_used")?),
                    i64::from(Score::IF),
                ],
                &[
                    condition.score.index(),
                    if_true.score.index(),
                    if_false.score.index(),
                    before_len,
                ],
            )?,
        ))
    } else {
        Ok((
            false,
            check_nonsemantic_component_append(scores, before_len)?,
        ))
    }
}

/// Derive Match's exact append from the pre-EOF target and ordered cases.
fn check_match_append<'src>(
    frame: &MessageFrame<'src>,
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
) -> Result<(bool, usize), CanonicalProtobufAdmissionError> {
    let target = optional_par(frame, "target")?;
    let mut all_required = true;
    let mut all_fixed = true;
    let child_capacity = semantic_slot(frame, "cases")?
        .len()
        .checked_add(1)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let mut children = Vec::with_capacity(child_capacity);
    if let Some((target, _)) = target {
        children.push(target.score.index());
    }
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
            children.push(score.index());
        }
    }
    if target.is_none() || !all_required {
        return Ok((
            false,
            check_nonsemantic_component_append(scores, before_len)?,
        ));
    }
    if !all_fixed {
        return Err(fixed_point_error(
            frame,
            "Match guard is a present semantic default",
        ));
    }
    let connective_used = bool_value(frame, "connective_used")?;
    let connective_index = before_len;
    children.push(connective_index);
    Ok((
        true,
        check_component_constructor_append(
            scores,
            before_len,
            &[i64::from(connective_used), i64::from(Score::MATCH)],
            &children,
        )?,
    ))
}

/// The bound index is a pre-EOF scalar; neither the reducer's output nor its
/// selected input is used to construct this expected score source.
fn check_cost_bound_append<'src>(
    frame: &MessageFrame<'src>,
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
) -> Result<usize, CanonicalProtobufAdmissionError> {
    check_component_constructor_append(
        scores,
        before_len,
        &[
            i64::from(i32_value(frame, "index")?),
            i64::from(Score::COST_BOUND),
        ],
        &[before_len],
    )
}

/// Check the exact ordered child-summary handles of a cost collection.
fn check_cost_list_append<'src>(
    frame: &MessageFrame<'src>,
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
    slot: &'static str,
    constructor: i32,
) -> Result<usize, CanonicalProtobufAdmissionError> {
    let occurrences = semantic_slot(frame, slot)?;
    let mut children = Vec::with_capacity(occurrences.len());
    for occurrence in occurrences {
        let summary = message_summary(occurrence, frame)?;
        let SemanticSummary::CostScore(score) = summary.semantic else {
            return Err(frame.schema_error_at(
                0,
                occurrence.field_position,
                "cost child returned a different semantic summary",
            ));
        };
        children.push(score.index());
    }
    check_component_constructor_append(scores, before_len, &[i64::from(constructor)], &children)
}

fn check_cost_signature_append<'src>(
    frame: &MessageFrame<'src>,
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
) -> Result<usize, CanonicalProtobufAdmissionError> {
    check_cost_list_append(frame, scores, before_len, "atoms", Score::COST_SIGNATURE)
}

fn check_cost_token_stack_append<'src>(
    frame: &MessageFrame<'src>,
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
) -> Result<usize, CanonicalProtobufAdmissionError> {
    check_cost_list_append(frame, scores, before_len, "layers", Score::COST_TOKEN_STACK)
}

/// A borrowed, forward-only view of entries appended by one EOF reducer.
/// It never constructs scores and requires every selected source entry to be
/// present in exactly the order prescribed by the pre-EOF semantic fields.
struct ScoreAppendSourceCursor<'arena, 'src> {
    scores: &'arena CanonicalScoreArena<'src>,
    next: usize,
    mismatch: &'static str,
}

impl<'arena, 'src> ScoreAppendSourceCursor<'arena, 'src> {
    fn new(
        scores: &'arena CanonicalScoreArena<'src>,
        before_len: usize,
        mismatch: &'static str,
    ) -> Self {
        Self {
            scores,
            next: before_len,
            mismatch,
        }
    }

    fn error(&self) -> CanonicalProtobufAdmissionError {
        CanonicalProtobufAdmissionError::CutoverInvariant(self.mismatch)
    }

    fn advance(&mut self) -> Result<usize, CanonicalProtobufAdmissionError> {
        let index = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        Ok(index)
    }

    fn integer(&mut self, expected: i64) -> Result<usize, CanonicalProtobufAdmissionError> {
        let index = self.next;
        if !self.scores.source_entry_at(index).is_some_and(|entry| {
            matches!(entry.kind, CanonicalScoreNodeView::Integer(actual) if actual == expected)
        }) {
            return Err(self.error());
        }
        self.advance()
    }

    fn indexed_string(
        &mut self,
        expected: &AdmissionRegion<'src>,
    ) -> Result<usize, CanonicalProtobufAdmissionError> {
        let index = self.next;
        if !self.scores.source_entry_at(index).is_some_and(|entry| {
            matches!(
                entry.kind,
                CanonicalScoreNodeView::String(CanonicalScoreBytesView::Indexed(source))
                    if source.same_view(expected)
            ) || matches!(
                entry.kind,
                CanonicalScoreNodeView::String(CanonicalScoreBytesView::BorrowedIndexed(source))
                    if expected.same_borrowed_window(source)
            )
        }) {
            return Err(self.error());
        }
        self.advance()
    }

    fn node(&mut self, expected: &[usize]) -> Result<usize, CanonicalProtobufAdmissionError> {
        let index = self.next;
        let Some(entry) = self.scores.source_entry_at(index) else {
            return Err(self.error());
        };
        let CanonicalScoreNodeView::Node(children) = entry.kind else {
            return Err(self.error());
        };
        if children.len() != expected.len()
            || !children
                .iter()
                .zip(expected)
                .all(|(actual, expected)| actual.index() == *expected)
        {
            return Err(self.error());
        }
        self.advance()
    }

    fn empty_par(&mut self) -> Result<usize, CanonicalProtobufAdmissionError> {
        let connective = self.integer(0)?;
        let constructor = self.integer(i64::from(Score::PAR))?;
        self.node(&[constructor, connective])
    }

    fn finish(self) -> Result<(), CanonicalProtobufAdmissionError> {
        if self.next == self.scores.source_len() {
            Ok(())
        } else {
            Err(self.error())
        }
    }
}

/// Independently project Receive's source from the already admitted child
/// summaries, including the optional empty-condition Par append.
fn check_receive_append<'src>(
    frame: &MessageFrame<'src>,
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
) -> Result<(bool, usize), CanonicalProtobufAdmissionError> {
    let body = optional_par(frame, "body")?;
    let mut bind_scores = Vec::with_capacity(semantic_slot(frame, "binds")?.len());
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
            bind_scores.push(score.index());
        }
    }
    let Some((body, _)) = body else {
        return Ok((
            false,
            check_nonsemantic_component_append(scores, before_len)?,
        ));
    };
    if !all_sources {
        return Ok((
            false,
            check_nonsemantic_component_append(scores, before_len)?,
        ));
    }
    let condition = optional_par(frame, "condition")?;
    if condition.is_some_and(|(par, _)| par.semantic_default) {
        return Err(fixed_point_error(
            frame,
            "Receive condition is a present semantic default",
        ));
    }
    let mut source = ScoreAppendSourceCursor::new(
        scores,
        before_len,
        "Receive EOF score append disagrees with pre-EOF semantic slots",
    );
    let condition_score = match condition {
        Some((condition, _)) => condition.score.index(),
        None => source.empty_par()?,
    };
    let persistent = source.integer(i64::from(bool_value(frame, "persistent")?))?;
    let peek = source.integer(i64::from(bool_value(frame, "peek")?))?;
    let bind_count = source.integer(i64::from(i32_value(frame, "bind_count")?))?;
    let connective = source.integer(i64::from(bool_value(frame, "connective_used")?))?;
    let constructor = source.integer(i64::from(Score::RECEIVE))?;
    let cost = cost_score(frame, "cost_region")?;
    let mut children = Vec::with_capacity(
        bind_scores
            .len()
            .checked_add(7)
            .and_then(|count| count.checked_add(usize::from(cost.is_some())))
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?,
    );
    children.extend([constructor, persistent, peek]);
    children.extend(bind_scores);
    children.extend([body.score.index(), bind_count, connective, condition_score]);
    if let Some(cost) = cost {
        children.push(cost.index());
    }
    let root = source.node(&children)?;
    source.finish()?;
    Ok((true, root))
}

/// Independently project New's ordered URI/injection source. The indexed
/// strings are matched by backing-view identity, not by reducer output.
fn check_new_append<'src>(
    frame: &MessageFrame<'src>,
    scores: &CanonicalScoreArena<'src>,
    before_len: usize,
) -> Result<(bool, usize), CanonicalProtobufAdmissionError> {
    let Some((body, _)) = optional_par(frame, "p")? else {
        return Ok((
            false,
            check_nonsemantic_component_append(scores, before_len)?,
        ));
    };
    let mut source = ScoreAppendSourceCursor::new(
        scores,
        before_len,
        "New EOF score append disagrees with pre-EOF semantic slots",
    );
    let uris = semantic_slot(frame, "uri")?;
    let mut uri_scores = Vec::with_capacity(uris.len().max(1));
    let mut previous_uri: Option<&AdmissionRegion<'src>> = None;
    for occurrence in uris {
        let SemanticValue::Scalar(SemanticScalar::String(uri)) = &occurrence.value else {
            return Err(frame.schema_error_at(
                3,
                occurrence.field_position,
                "New URI returned a different semantic scalar",
            ));
        };
        if previous_uri.is_some_and(|previous| previous.compare(uri).is_gt()) {
            return Err(frame.schema_error_at(
                3,
                occurrence.field_position,
                "New URIs are outside canonical string order",
            ));
        }
        previous_uri = Some(uri);
        uri_scores.push(source.indexed_string(uri)?);
    }
    if uri_scores.is_empty() {
        uri_scores.push(source.integer(i64::from(Score::ABSENT))?);
    }
    let injections = semantic_slot(frame, "injections")?;
    let mut injection_scores = Vec::with_capacity(injections.len().max(1));
    for occurrence in injections {
        let SemanticValue::Map(entry) = &occurrence.value else {
            return Err(frame.schema_error_at(
                4,
                occurrence.field_position,
                "New injection returned a different semantic summary",
            ));
        };
        let value_score = match &entry.value {
            Some(value) => value.score.index(),
            None => source.empty_par()?,
        };
        let key = source.indexed_string(&entry.key)?;
        injection_scores.push(source.node(&[key, value_score])?);
    }
    if injection_scores.is_empty() {
        injection_scores.push(source.integer(i64::from(Score::ABSENT))?);
    }
    let bind_count = source.integer(i64::from(i32_value(frame, "bind_count")?))?;
    let constructor = source.integer(i64::from(Score::NEW))?;
    let children_capacity = uri_scores
        .len()
        .checked_add(injection_scores.len())
        .and_then(|count| count.checked_add(3))
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let mut children = Vec::with_capacity(children_capacity);
    children.extend([constructor, bind_count]);
    children.extend(uri_scores);
    children.extend(injection_scores);
    children.push(body.score.index());
    let root = source.node(&children)?;
    source.finish()?;
    Ok((true, root))
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReceiveFinishInput {
    Incomplete,
    Complete {
        persistent: bool,
        peek: bool,
        binds: Vec<CanonicalScoreHandle>,
        body: CanonicalScoreHandle,
        bind_count: i32,
        connective_used: bool,
        condition: Option<CanonicalScoreHandle>,
        cost: Option<CanonicalScoreHandle>,
    },
}

fn reduce_component_receive<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    #[cfg(test)]
    let source_len_before = scores.source_len();
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
    #[cfg(test)]
    let input;
    let result = if !complete {
        #[cfg(test)]
        {
            input = ReceiveFinishInput::Incomplete;
        }
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
        let persistent = bool_value(frame, "persistent")?;
        let peek = bool_value(frame, "peek")?;
        let body_score = body.expect("complete Receive has a body").0.score;
        let bind_count = i32_value(frame, "bind_count")?;
        let connective_used = bool_value(frame, "connective_used")?;
        let cost = cost_score(frame, "cost_region")?;
        #[cfg(test)]
        {
            input = ReceiveFinishInput::Complete {
                persistent,
                peek,
                binds: binds.clone(),
                body: body_score,
                bind_count,
                connective_used,
                condition: condition.map(|(condition, _)| condition.score),
                cost,
            };
        }
        (
            true,
            semantics::receive_score(
                scores,
                persistent,
                peek,
                binds,
                body_score,
                bind_count,
                connective_used,
                condition_score,
                cost,
            )?,
        )
    };
    #[cfg(test)]
    rpb3_root_finish_trace::record_receive_finish(
        frame,
        input,
        result,
        source_len_before,
        scores.source_len(),
    );
    Ok(result)
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum NewFinishInput {
    Incomplete,
    Complete {
        body: CanonicalScoreHandle,
        bind_count: i32,
        uris: Vec<Vec<u8>>,
        injections: Vec<(Vec<u8>, Option<CanonicalScoreHandle>)>,
    },
}

fn reduce_component_new<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    #[cfg(test)]
    let source_len_before = scores.source_len();
    #[cfg(test)]
    let input;
    let result = {
        let body = optional_par(frame, "p")?;
        if body.is_none() {
            #[cfg(test)]
            {
                input = NewFinishInput::Incomplete;
            }
            (false, nonsemantic_score(scores)?)
        } else {
            let uris = semantic_slot(frame, "uri")?;
            let mut uri_scores = Vec::new();
            #[cfg(test)]
            let mut uri_values = Vec::new();
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
                #[cfg(test)]
                uri_values.push(uri.bytes().collect());
                uri_scores.push(scores.indexed_string(uri.clone())?.into_score());
            }
            if uri_scores.is_empty() {
                uri_scores.push(scores.integer(i64::from(Score::ABSENT))?);
            }

            let mut injection_scores = Vec::new();
            #[cfg(test)]
            let mut injection_values = Vec::new();
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
                #[cfg(test)]
                injection_values.push((
                    entry.key.bytes().collect(),
                    entry.value.as_ref().map(|value| value.score),
                ));
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
            let bind_count = i32_value(frame, "bind_count")?;
            let body_score = body.expect("complete New has a body").0.score;
            #[cfg(test)]
            {
                input = NewFinishInput::Complete {
                    body: body_score,
                    bind_count,
                    uris: uri_values,
                    injections: injection_values,
                };
            }
            (
                true,
                semantics::new_score(scores, bind_count, uri_scores, injection_scores, body_score)?,
            )
        }
    };
    #[cfg(test)]
    rpb3_root_finish_trace::record_new_finish(
        frame,
        input,
        result,
        source_len_before,
        scores.source_len(),
    );
    Ok(result)
}

#[cfg_attr(test, derive(Clone, Debug, PartialEq, Eq))]
enum MatchFinishInput {
    Incomplete,
    Complete {
        target: CanonicalScoreHandle,
        cases: Vec<CanonicalScoreHandle>,
        connective_used: bool,
    },
}

fn match_finish_input<'src>(
    frame: &MessageFrame<'src>,
) -> Result<MatchFinishInput, CanonicalProtobufAdmissionError> {
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
    if target.is_none() || !all_required {
        return Ok(MatchFinishInput::Incomplete);
    }
    if !all_fixed {
        return Err(fixed_point_error(
            frame,
            "Match guard is a present semantic default",
        ));
    }
    Ok(MatchFinishInput::Complete {
        target: target.expect("complete Match has a target").0.score,
        cases,
        connective_used: bool_value(frame, "connective_used")?,
    })
}

fn reduce_component_match<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    let input = match_finish_input(frame)?;
    #[cfg(test)]
    let source_input = input.clone();
    #[cfg(test)]
    let source_len_before = scores.source_len();
    let result = match input {
        MatchFinishInput::Incomplete => (false, nonsemantic_score(scores)?),
        MatchFinishInput::Complete {
            target,
            cases,
            connective_used,
        } => (
            true,
            semantics::match_score(scores, target, cases, connective_used)?,
        ),
    };
    #[cfg(test)]
    rpb3_root_finish_trace::record_match_finish(
        frame,
        source_input,
        result,
        source_len_before,
        scores.source_len(),
    );
    Ok(result)
}

#[cfg_attr(test, derive(Clone, Debug, PartialEq, Eq))]
enum BundleFinishInput {
    Incomplete,
    Complete {
        read: bool,
        write: bool,
        body: CanonicalScoreHandle,
    },
}

fn bundle_finish_input<'src>(
    frame: &MessageFrame<'src>,
) -> Result<BundleFinishInput, CanonicalProtobufAdmissionError> {
    let Some((body, _)) = optional_par(frame, "body")? else {
        return Ok(BundleFinishInput::Incomplete);
    };
    Ok(BundleFinishInput::Complete {
        read: bool_value(frame, "read_flag")?,
        write: bool_value(frame, "write_flag")?,
        body: body.score,
    })
}

fn reduce_component_bundle<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    let input = bundle_finish_input(frame)?;
    #[cfg(test)]
    let source_input = input.clone();
    #[cfg(test)]
    let source_len_before = scores.source_len();
    let result = match input {
        BundleFinishInput::Incomplete => (false, nonsemantic_score(scores)?),
        BundleFinishInput::Complete { read, write, body } => {
            (true, semantics::bundle_score(scores, read, write, body)?)
        }
    };
    #[cfg(test)]
    rpb3_root_finish_trace::record_bundle_finish(
        frame,
        source_input,
        result,
        source_len_before,
        scores.source_len(),
    );
    Ok(result)
}

#[cfg_attr(test, derive(Clone, Debug, PartialEq, Eq))]
enum IfFinishInput {
    Incomplete,
    Complete {
        condition: CanonicalScoreHandle,
        if_true: CanonicalScoreHandle,
        if_false: CanonicalScoreHandle,
        connective_used: bool,
    },
}

fn if_finish_input<'src>(
    frame: &MessageFrame<'src>,
) -> Result<IfFinishInput, CanonicalProtobufAdmissionError> {
    let condition = optional_par(frame, "condition")?;
    let if_true = optional_par(frame, "if_true")?;
    let if_false = optional_par(frame, "if_false")?;
    if let (Some((condition, _)), Some((if_true, _)), Some((if_false, _))) =
        (condition, if_true, if_false)
    {
        Ok(IfFinishInput::Complete {
            condition: condition.score,
            if_true: if_true.score,
            if_false: if_false.score,
            connective_used: bool_value(frame, "connective_used")?,
        })
    } else {
        Ok(IfFinishInput::Incomplete)
    }
}

fn reduce_component_if<'src>(
    frame: &MessageFrame<'src>,
    scores: &mut CanonicalScoreArena<'src>,
) -> Result<(bool, CanonicalScoreHandle), CanonicalProtobufAdmissionError> {
    let input = if_finish_input(frame)?;
    #[cfg(test)]
    let source_input = input.clone();
    #[cfg(test)]
    let source_len_before = scores.source_len();
    let result = match input {
        IfFinishInput::Incomplete => (false, nonsemantic_score(scores)?),
        IfFinishInput::Complete {
            condition,
            if_true,
            if_false,
            connective_used,
        } => (
            true,
            semantics::if_score(scores, condition, if_true, if_false, connective_used)?,
        ),
    };
    #[cfg(test)]
    rpb3_root_finish_trace::record_if_finish(
        frame,
        source_input,
        result,
        source_len_before,
        scores.source_len(),
    );
    Ok(result)
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
    #[cfg(test)]
    let source_len_before = scores.source_len();
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
    #[cfg(test)]
    rpb3_root_finish_trace::record_expr_finish(
        frame,
        score,
        complete,
        stable,
        source_len_before,
        scores.source_len(),
    );
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
    #[cfg(test)]
    let source_len_before = scores.source_len();
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

    #[cfg(test)]
    let source_pair_scores = pair_scores.clone();
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
    #[cfg(test)]
    rpb3_root_finish_trace::record_emap_finish(
        frame,
        source_pair_scores,
        complete,
        canonical_fixed_point,
        score,
        source_len_before,
        scores.source_len(),
    );
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
    #[cfg(test)]
    let source_len_before = scores.source_len();
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
    #[cfg(test)]
    rpb3_root_finish_trace::record_epathmap_score(
        frame,
        &snapshot.snapshot,
        snapshot_score,
        score,
        source_len_before,
        scores.source_len(),
    );
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
        #[cfg(test)]
        rpb3_root_finish_trace::record_expr_payload_finish(
            frame,
            None,
            scores.source_len(),
            scores.source_len(),
        );
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
    #[cfg(test)]
    let source_len_before = scores.source_len();
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
    #[cfg(test)]
    rpb3_root_finish_trace::record_expr_payload_finish(
        frame,
        score,
        source_len_before,
        scores.source_len(),
    );
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
        let source = MapAcceptObservation {
            score: value.score,
            canonical_path_stable: value.canonical_path_stable,
            identity_prefix_len: value.identity_prefix.len(),
            identity_suffix_len: value.identity_suffix.len(),
            value_position: self.value_position,
        };
        self.value = Some(value);
        if !source.matches_frame(self) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "map value return disagrees with its source observation",
            ));
        }
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
    source_view: Option<SourceTreeWindow<'src>>,
    path: CanonicalPathTopologyAdmission<'src>,
    mode: EPathMapMode,
    entries_canonical_path_stable: bool,
    waiting: Option<EPathMapWait>,
    waiting_child: Option<AdmissionRegion<'src>>,
    last_source: CanonicalPathSource,
    terminal_source: Option<Box<EPathMapTerminalSource<'src>>>,
}

/// Retained, constant-size facts from the successful indexed EPM1 EOF arm.
/// Construction follows the cursor's `None` result and one successful path
/// finish, so no second cursor or reducer call is needed to observe either.
struct EPathMapTerminalSource<'src> {
    snapshot: AdmissionRegion<'src>,
    region_start: CanonicalByteProvenance,
    end: CanonicalByteProvenance,
    cursor: IndexedCanonicalEpm1TopologyStats,
    cursor_source: IndexedCanonicalEpm1TerminalSourceImage,
    last_source: CanonicalPathSource,
    mode: EPathMapMode,
    entries_canonical_path_stable: bool,
    protobuf_read_before: usize,
    protobuf_traversed_before: usize,
    events_before: usize,
}

impl EPathMapTerminalSource<'_> {
    fn matches_complete(
        &self,
        frame: &EPathMapFrame<'_>,
        summary: &FrameSummary<'_>,
        stats: &CanonicalProtobufAdmissionStats,
    ) -> bool {
        self.snapshot.same_view(&frame.snapshot)
            && self.region_start == frame.region_start
            && frame
                .snapshot
                .position_at(0)
                .map(|position| position.physical)
                == Some(self.region_start)
            && self.end == frame.snapshot.end_position().physical
            && self.cursor == frame.cursor.stats()
            && frame.cursor.terminal_source_image() == Some(self.cursor_source)
            && self.cursor_source.snapshot_len == frame.snapshot.len()
            && self.cursor_source.event_index == self.cursor.topology_events
            && self.cursor_source.map_values_seen == self.cursor.value_records
            && self.cursor_source.terminal_checks == self.cursor.terminal_checks
            && self.last_source == frame.last_source
            && frame.waiting.is_none()
            && frame.waiting_child.is_none()
            && frame.path.active_obligation().is_none()
            && self.mode == frame.mode
            && self.mode == frame.cursor.mode()
            && self.entries_canonical_path_stable == frame.entries_canonical_path_stable
            && self
                .protobuf_read_before
                .checked_add(self.cursor.prefix_bytes_read)
                .and_then(|count| count.checked_add(self.cursor.value_table_bytes_read))
                == Some(stats.protobuf_bytes_read)
            && self
                .protobuf_traversed_before
                .checked_add(self.cursor.prefix_bytes_traversed)
                .and_then(|count| count.checked_add(self.cursor.value_table_bytes_traversed))
                == Some(stats.protobuf_bytes_traversed)
            && self.events_before.checked_add(self.cursor.topology_events)
                == Some(stats.epathmap_events)
            && matches!(summary, FrameSummary::EPathMap(value)
                if value.snapshot.same_view(&self.snapshot)
                    && value.mode == self.mode
                    && value.entries_canonical_path_stable == self.entries_canonical_path_stable)
    }
}

enum EPathMapWait {
    MapValue,
    Obligation(PathObligationWait),
}

struct EPathMapMapValueAcceptObservation {
    prior_waiting_map_value: bool,
    returned_canonical_path_stable: bool,
    entries_before: bool,
    cursor_before: IndexedCanonicalEpm1TopologyStats,
    active_obligation_before: Option<CanonicalPathObligationProgress>,
    mode_before: EPathMapMode,
    last_source_before: CanonicalPathSource,
}

impl EPathMapMapValueAcceptObservation {
    fn matches_frame(&self, frame: &EPathMapFrame<'_>) -> bool {
        self.prior_waiting_map_value
            && frame.waiting.is_none()
            && frame.entries_canonical_path_stable
                == (self.entries_before && self.returned_canonical_path_stable)
            && frame.cursor.stats() == self.cursor_before
            && frame.path.active_obligation() == self.active_obligation_before
            && frame.mode == self.mode_before
            && frame.last_source == self.last_source_before
    }
}

struct PathObligationWait {
    id: CanonicalPathObligationId,
    kind: CanonicalPathObligationKind,
}

impl PathObligationWait {
    fn resolution(&self, summary: &FrameSummary<'_>) -> Option<CanonicalPathObligationResolution> {
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
    waiting_child: Option<AdmissionRegion<'src>>,
    terminal_source: Option<CanonicalPathTerminalSource<'src>>,
}

struct CanonicalPathTerminalSource<'src> {
    input: AdmissionRegion<'src>,
    end: CanonicalByteProvenance,
    bytes_yielded: usize,
    bytes_traversed: usize,
    bytes_read_before: usize,
    bytes_traversed_before: usize,
    summary: CanonicalPathSummary,
}

impl CanonicalPathTerminalSource<'_> {
    fn matches_complete(
        &self,
        frame: &CanonicalPathFrame<'_>,
        summary: &FrameSummary<'_>,
        stats: &CanonicalProtobufAdmissionStats,
    ) -> bool {
        self.input.same_view(&frame.input)
            && self.end == frame.input.end_position().physical
            && self.bytes_yielded == frame.cursor.bytes_yielded()
            && self.bytes_traversed == frame.cursor.bytes_traversed()
            && frame.waiting.is_none()
            && frame.waiting_child.is_none()
            && self.bytes_read_before.checked_add(self.bytes_yielded)
                == Some(stats.canonical_path_bytes_read)
            && self
                .bytes_traversed_before
                .checked_add(self.bytes_traversed)
                == Some(stats.canonical_path_bytes_traversed)
            && matches!(summary, FrameSummary::CanonicalPath(value) if *value == self.summary)
    }
}

impl<'src> CanonicalPathFrame<'src> {
    fn new(input: AdmissionRegion<'src>) -> Self {
        let reducer = CanonicalPathStreamReducer::new();
        Self {
            cursor: input.located_bytes(),
            input,
            reducer,
            waiting: None,
            waiting_child: None,
            terminal_source: None,
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

    fn matches_push_request(&self, request: &FrameRequest<'src>) -> bool {
        let (Some(waiting), Some(selected)) = (&self.waiting, &self.waiting_child) else {
            return false;
        };
        if self.terminal_source.is_some() || !selected.same_view(request.region()) {
            return false;
        }
        let obligation_ready = self.reducer.active_obligation().is_some_and(|progress| {
            progress.obligation.id == waiting.id
                && progress.obligation.kind == waiting.kind
                && progress.awaiting_resolution
                && progress.remaining == 0
                && u64::try_from(selected.len()).ok() == Some(progress.obligation.payload_len)
        });
        if !obligation_ready {
            return false;
        }
        match (waiting.kind, request) {
            (CanonicalPathObligationKind::RawParEscape, FrameRequest::Message { program, .. }) => {
                std::ptr::eq(*program, &PAR_PROTOBUF_REGION_PROGRAM)
            }
            (
                CanonicalPathObligationKind::NestedEpm1,
                FrameRequest::EPathMap { owner, field, .. },
            ) => std::ptr::eq(*owner, &PAR_PROTOBUF_REGION_PROGRAM) && *field == 0,
            _ => false,
        }
    }

    fn accept(
        &mut self,
        summary: FrameSummary<'src>,
        stats: &mut CanonicalProtobufAdmissionStats,
    ) -> Result<FrameAcceptEffect<'src>, CanonicalProtobufAdmissionError> {
        let waiting = self.waiting.take().ok_or_else(|| {
            self.protocol_error("canonical path child returned without a continuation")
        })?;
        self.waiting_child.take().ok_or_else(|| {
            self.protocol_error("canonical path child returned without a retained source view")
        })?;
        let resolution = waiting.resolution(&summary).ok_or_else(|| {
            self.protocol_error("canonical path child returned the wrong summary kind")
        })?;
        self.reducer
            .resolve_obligation(&self.input, waiting.id, resolution)
            .map_err(Self::path_error)?;
        stats.canonical_path_resolutions = stats
            .canonical_path_resolutions
            .checked_add(1)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        Ok(FrameAcceptEffect::ResolvedObligation {
            id: waiting.id,
            resolution,
        })
    }

    fn step(
        &mut self,
        stats: &mut CanonicalProtobufAdmissionStats,
    ) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
        if self.waiting.is_some() || self.waiting_child.is_some() || self.terminal_source.is_some()
        {
            return Err(self.protocol_error("canonical path resumed before its child returned"));
        }
        if self.cursor.bytes_yielded() != self.reducer.consumed_len() {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "canonical path cursor and reducer consumed different input prefixes",
            ));
        }
        while let Some(byte) = self.cursor.next() {
            #[cfg(test)]
            let source_index = self.cursor.bytes_yielded() - 1;
            #[cfg(test)]
            let mut skipped = 0;
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
                    #[cfg(test)]
                    {
                        skipped = plan.width;
                    }
                }
            }
            if self.cursor.bytes_yielded() != self.reducer.consumed_len() {
                return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                    "canonical path cursor and reducer consumed different input prefixes",
                ));
            }
            #[cfg(test)]
            rpb3_root_finish_trace::record_canonical_path_byte(self, source_index, byte, skipped);
            if let Some(completed) = self
                .reducer
                .completed_obligation(&self.input)
                .map_err(Self::path_error)?
            {
                // An empty child has no payload byte at `payload.end`; the
                // reducer retains the physical length-byte anchor for it.
                let end = CanonicalByteProvenance::new(
                    completed.payload_end.source,
                    completed.payload_end.offset,
                );
                let body = self
                    .input
                    .subview_with_end(completed.payload, end)
                    .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
                let (waiting, frame) = obligation_region_frame(
                    completed.obligation,
                    body.clone(),
                    &PAR_PROTOBUF_REGION_PROGRAM,
                    0,
                    stats,
                )?;
                self.waiting = Some(waiting);
                self.waiting_child = Some(body);
                return Ok(FrameAction::Push(frame));
            }
        }
        let end = self.input.end_position().physical;
        let bytes_read_before = stats.canonical_path_bytes_read;
        let bytes_traversed_before = stats.canonical_path_bytes_traversed;
        let summary = self
            .reducer
            .value(true, CanonicalPathSource::in_source(end.source, end.offset))
            .expect("standalone canonical path owns one endpoint")
            .map_err(Self::path_error)?;
        stats.canonical_path_bytes_read = bytes_read_before
            .checked_add(self.cursor.bytes_yielded())
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        stats.canonical_path_bytes_traversed = bytes_traversed_before
            .checked_add(self.cursor.bytes_traversed())
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        let source = CanonicalPathTerminalSource {
            input: self.input.clone(),
            end,
            bytes_yielded: self.cursor.bytes_yielded(),
            bytes_traversed: self.cursor.bytes_traversed(),
            bytes_read_before,
            bytes_traversed_before,
            summary,
        };
        let action = FrameAction::Complete(FrameSummary::CanonicalPath(summary));
        let FrameAction::Complete(returned) = &action else {
            unreachable!()
        };
        if !source.matches_complete(self, returned, stats) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "canonical path terminal return disagrees with its source observation",
            ));
        }
        self.terminal_source = Some(source);
        Ok(action)
    }
}

impl<'src> EPathMapFrame<'src> {
    fn prepare_epm1_cursor(
        owner: &'static RhoMatchProgram,
        field: u32,
        snapshot: &AdmissionRegion<'src>,
    ) -> Result<
        (
            CanonicalByteProvenance,
            IndexedCanonicalEpm1TopologyCursor<'src>,
        ),
        CanonicalProtobufAdmissionError,
    > {
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
        Ok((region_start, cursor))
    }

    fn with_prepared_epm1_region<T>(
        owner: &'static RhoMatchProgram,
        field: u32,
        snapshot: AdmissionRegion<'src>,
        region_start: CanonicalByteProvenance,
        cursor: IndexedCanonicalEpm1TopologyCursor<'src>,
        finish: impl FnOnce(Self) -> Result<T, CanonicalProtobufAdmissionError>,
    ) -> Result<T, CanonicalProtobufAdmissionError> {
        let mode = cursor.mode();
        let source_view = cursor.source_window();
        let path = CanonicalPathTopologyAdmission::new_with_source(source_view.clone())
            .map_err(|_| CanonicalProtobufAdmissionError::SizeOverflow)?;
        finish(Self {
            owner,
            field,
            region_start,
            snapshot,
            cursor,
            source_view,
            path,
            mode,
            entries_canonical_path_stable: true,
            waiting: None,
            waiting_child: None,
            last_source: CanonicalPathSource::in_source(region_start.source, region_start.offset),
            terminal_source: None,
        })
    }

    #[cfg(test)]
    fn with_epm1_region<T>(
        owner: &'static RhoMatchProgram,
        field: u32,
        snapshot: AdmissionRegion<'src>,
        finish: impl FnOnce(Self) -> Result<T, CanonicalProtobufAdmissionError>,
    ) -> Result<T, CanonicalProtobufAdmissionError> {
        let (region_start, cursor) = Self::prepare_epm1_cursor(owner, field, &snapshot)?;
        Self::with_prepared_epm1_region(owner, field, snapshot, region_start, cursor, finish)
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
    fn matches_push_request(&self, request: &FrameRequest<'src>) -> bool {
        let Some(selected) = self.waiting_child.as_ref() else {
            return false;
        };
        if self.terminal_source.is_some() || !selected.same_view(request.region()) {
            return false;
        }
        let obligation_ready = |waiting: &PathObligationWait| {
            self.path.active_obligation().is_some_and(|progress| {
                progress.obligation.id == waiting.id
                    && progress.obligation.kind == waiting.kind
                    && progress.awaiting_resolution
                    && progress.remaining == 0
                    && u64::try_from(selected.len()).ok() == Some(progress.obligation.payload_len)
            })
        };
        match (&self.waiting, request) {
            (Some(EPathMapWait::MapValue), FrameRequest::Message { program, .. }) => {
                self.mode == EPathMapMode::Map
                    && std::ptr::eq(*program, &PAR_PROTOBUF_REGION_PROGRAM)
                    && self.path.active_obligation().is_none()
                    && self.cursor.stats().value_records > 0
            }
            (Some(EPathMapWait::Obligation(waiting)), FrameRequest::Message { program, .. }) => {
                waiting.kind == CanonicalPathObligationKind::RawParEscape
                    && std::ptr::eq(*program, &PAR_PROTOBUF_REGION_PROGRAM)
                    && obligation_ready(waiting)
            }
            (
                Some(EPathMapWait::Obligation(waiting)),
                FrameRequest::EPathMap { owner, field, .. },
            ) => {
                waiting.kind == CanonicalPathObligationKind::NestedEpm1
                    && std::ptr::eq(*owner, self.owner)
                    && *field == self.field
                    && obligation_ready(waiting)
            }
            _ => false,
        }
    }

    fn accept(
        &mut self,
        summary: FrameSummary<'src>,
        stats: &mut CanonicalProtobufAdmissionStats,
    ) -> Result<FrameAcceptEffect<'src>, CanonicalProtobufAdmissionError> {
        let prior_waiting_map_value = matches!(self.waiting.as_ref(), Some(EPathMapWait::MapValue));
        let Some(waiting) = self.waiting.take() else {
            return Err(CanonicalProtobufAdmissionError::Schema {
                type_name: self.owner.type_name,
                field: self.field,
                position: self.region_start,
                detail: "EPM1 value returned without a continuation",
            });
        };
        if self.waiting_child.take().is_none() {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "EPM1 child returned without a retained source view",
            ));
        }
        match (waiting, summary) {
            (
                EPathMapWait::MapValue,
                FrameSummary::Message(MessageSummary {
                    semantic: SemanticSummary::Par(par),
                }),
            ) => {
                let returned = ParSummarySource::before(&par);
                let source = EPathMapMapValueAcceptObservation {
                    prior_waiting_map_value,
                    returned_canonical_path_stable: par.canonical_path_stable,
                    entries_before: self.entries_canonical_path_stable,
                    cursor_before: self.cursor.stats(),
                    active_obligation_before: self.path.active_obligation(),
                    mode_before: self.mode,
                    last_source_before: self.last_source,
                };
                self.entries_canonical_path_stable &= par.canonical_path_stable;
                if !source.matches_frame(self) {
                    return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                        "EPM1 map value return disagrees with its source observation",
                    ));
                }
                Ok(FrameAcceptEffect::EPathMapValue(returned))
            }
            (EPathMapWait::Obligation(waiting), summary) => {
                let resolution = waiting.resolution(&summary).ok_or(
                    CanonicalProtobufAdmissionError::Schema {
                        type_name: self.owner.type_name,
                        field: self.field,
                        position: self.region_start,
                        detail: "EPM1 child returned the wrong summary for its continuation",
                    },
                )?;
                self.path
                    .resolve_obligation(waiting.id, resolution)
                    .map_err(|source| self.path_error(source))?;
                stats.canonical_path_resolutions = stats
                    .canonical_path_resolutions
                    .checked_add(1)
                    .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
                Ok(FrameAcceptEffect::ResolvedObligation {
                    id: waiting.id,
                    resolution,
                })
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
    fn request_kind(&self) -> FrameRequestKind {
        match self {
            Self::Message(_) => FrameRequestKind::Message,
            Self::Map(_) => FrameRequestKind::Map,
            Self::EPathMap(_) => FrameRequestKind::EPathMap,
            Self::CanonicalPath(_) => FrameRequestKind::CanonicalPath,
        }
    }

    // Each step writes its result into the driver's heap scratch before any
    // push or completion phase starts.
    #[inline(never)]
    fn step_into(
        &mut self,
        stats: &mut CanonicalProtobufAdmissionStats,
        scores: &mut CanonicalScoreArena<'src>,
        scratch: &mut AdmissionScratch<'src>,
    ) -> Result<AdmissionStepKind, CanonicalProtobufAdmissionError> {
        match self {
            Self::Message(frame) => message_step_into(frame, stats, scores, scratch),
            Self::Map(frame) => map_frame_step_into(frame, stats, scratch),
            Self::EPathMap(frame) => epathmap_frame_step_into(frame, stats, scratch),
            Self::CanonicalPath(frame) => canonical_path_frame_step_into(frame, stats, scratch),
        }
    }

    #[cfg(test)]
    fn step(
        &mut self,
        stats: &mut CanonicalProtobufAdmissionStats,
        scores: &mut CanonicalScoreArena<'src>,
    ) -> Result<FrameStep<'src>, CanonicalProtobufAdmissionError> {
        let mut scratch = AdmissionScratch::allocate();
        let kind = self.step_into(stats, scores, &mut scratch)?;
        Ok(scratch.take_test_step(kind))
    }

    fn accept(
        &mut self,
        summary: FrameSummary<'src>,
        stats: &mut CanonicalProtobufAdmissionStats,
        scores: &CanonicalScoreArena<'src>,
    ) -> Result<FrameAcceptEffect<'src>, CanonicalProtobufAdmissionError> {
        match self {
            Self::Message(frame) => frame
                .accept(summary, scores)
                .map(|()| FrameAcceptEffect::Stored),
            Self::Map(frame) => frame.accept(summary).map(|()| FrameAcceptEffect::Stored),
            Self::EPathMap(frame) => frame.accept(summary, stats),
            Self::CanonicalPath(frame) => frame.accept(summary, stats),
        }
    }
}

#[inline(never)]
fn map_frame_step_into<'src>(
    frame: &mut MapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<AdmissionStepKind, CanonicalProtobufAdmissionError> {
    let (action, source) = map_step_with_source(frame, stats)?;
    Ok(scratch.store_other(action, Some(source)))
}

#[inline(never)]
fn epathmap_frame_step_into<'src>(
    frame: &mut EPathMapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<AdmissionStepKind, CanonicalProtobufAdmissionError> {
    let step = epathmap_step_into(frame, stats, scratch);
    #[cfg(test)]
    standalone_path_admission::record_full_admission_path(&frame.path);
    step
}

#[inline(never)]
fn canonical_path_frame_step_into<'src>(
    frame: &mut CanonicalPathFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<AdmissionStepKind, CanonicalProtobufAdmissionError> {
    Ok(scratch.store_other(frame.step(stats)?, None))
}

#[derive(Clone)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameRequestKind {
    Message,
    Map,
    EPathMap,
    CanonicalPath,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FramePushObservation {
    request_kind: FrameRequestKind,
    before_len: usize,
    before_capacity: usize,
    after_len: usize,
    after_capacity: usize,
    frame_variant_bytes: usize,
    stats_frame_variant_bytes: usize,
    used_bytes: usize,
    reserved_bytes: usize,
}

/// The scalar source facts available before the first frame push. The push
/// observation is returned only after `matches_pushed_frame` has checked the
/// complete region with `same_view`. The CanonicalPath root also retains the
/// selected indexed view as a constant-size source handle.
struct RootInitializationSource<'src> {
    request_kind: FrameRequestKind,
    program: Option<&'static RhoMatchProgram>,
    program_type_id: Option<u16>,
    // The request region is authoritative; `drive_admission` also accepts an
    // operational input-byte count that can be zero for a standalone child.
    body_len: usize,
    region_start: Option<CanonicalByteProvenance>,
    slot_count: usize,
    score_entries: usize,
    canonical_path_input: Option<AdmissionRegion<'src>>,
}

impl<'src> RootInitializationSource<'src> {
    fn before_push(
        request: &FrameRequest<'src>,
        scores: &CanonicalScoreArena<'src>,
    ) -> Option<Self> {
        let (program, program_type_id, region_start, slot_count, canonical_path_input) =
            match request {
                FrameRequest::Message { region, program } => (
                    Some(*program),
                    Some(program.type_id),
                    region.position_at(0).map(|position| position.physical),
                    program.fields.len(),
                    None,
                ),
                FrameRequest::CanonicalPath(region) => (None, None, None, 0, Some(region.clone())),
                FrameRequest::Map { .. } | FrameRequest::EPathMap { .. } => return None,
            };
        Some(Self {
            request_kind: request.request_kind(),
            program,
            program_type_id,
            body_len: request.region().len(),
            region_start,
            slot_count,
            score_entries: scores.source_len(),
            canonical_path_input,
        })
    }

    fn matches_root(
        &self,
        pushed: &FramePushObservation,
        frames: &[Frame<'_>],
        scores: &CanonicalScoreArena<'_>,
    ) -> bool {
        if pushed.request_kind != self.request_kind
            || pushed.before_len != 0
            || pushed.after_len != 1
            || frames.len() != 1
            || self.score_entries != 0
            || scores.source_len() != self.score_entries
        {
            return false;
        }
        match (frames.first(), self.program) {
            (Some(Frame::Message(frame)), Some(program)) => {
                std::ptr::eq(frame.program, program)
                    && Some(frame.program.type_id) == self.program_type_id
                    && frame.body.len() == self.body_len
                    && Some(frame.region_start) == self.region_start
                    && frame.cursor.matches_initial_view(&frame.body, false)
                    && frame.slots.last_slot().is_none()
                    && frame.slots.len() == self.slot_count
                    && frame.slots.seen_slice().iter().all(|seen| !seen)
                    && frame.semantic_slots.len() == self.slot_count
                    && frame.semantic_slots.iter().all(Vec::is_empty)
                    && frame.waiting.is_none()
                    && frame.par_components == 0
                    && frame.par_locally_free_range.is_none()
            }
            (Some(Frame::CanonicalPath(frame)), None) => {
                self.program_type_id.is_none()
                    && self.region_start.is_none()
                    && self.slot_count == 0
                    && self
                        .canonical_path_input
                        .as_ref()
                        .is_some_and(|selected| selected.same_view(&frame.input))
                    && frame.input.len() == self.body_len
                    && frame.cursor.bytes_yielded() == 0
                    && frame.cursor.bytes_traversed() == 0
                    && frame.reducer.active_obligation().is_none()
                    && frame.waiting.is_none()
                    && frame.waiting_child.is_none()
                    && frame.terminal_source.is_none()
            }
            _ => false,
        }
    }
}

impl FramePushObservation {
    fn after_push<'src>(
        request_kind: FrameRequestKind,
        before_len: usize,
        before_capacity: usize,
        frames: &Vec<Frame<'src>>,
        stats: &mut CanonicalProtobufAdmissionStats,
    ) -> Result<Self, CanonicalProtobufAdmissionError> {
        let after_len = frames.len();
        let after_capacity = frames.capacity();
        let frame_variant_bytes = std::mem::size_of::<Frame>();
        stats.max_frames = stats.max_frames.max(after_len);
        let used_bytes = after_len
            .checked_mul(frame_variant_bytes)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        stats.max_frame_storage_bytes = stats.max_frame_storage_bytes.max(used_bytes);
        let reserved_bytes = after_capacity
            .checked_mul(frame_variant_bytes)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        stats.max_frame_capacity_bytes = stats.max_frame_capacity_bytes.max(reserved_bytes);
        if before_len.checked_add(1) != Some(after_len)
            || after_capacity < after_len
            || after_capacity < before_capacity
            || frames.last().map(Frame::request_kind) != Some(request_kind)
        {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "frame request push did not publish one matching frame",
            ));
        }
        Ok(Self {
            request_kind,
            before_len,
            before_capacity,
            after_len,
            after_capacity,
            frame_variant_bytes,
            stats_frame_variant_bytes: stats.frame_variant_bytes,
            used_bytes,
            reserved_bytes,
        })
    }
}

impl<'src> FrameRequest<'src> {
    fn matches_pushed_frame(&self, frame: &Frame<'src>) -> bool {
        match (self, frame) {
            (Self::Message { region, program }, Frame::Message(pushed)) => {
                region.same_view(&pushed.body)
                    && std::ptr::eq(*program, pushed.program)
                    && region.position_at(0).map(|position| position.physical)
                        == Some(pushed.region_start)
                    && pushed.cursor.matches_initial_view(&pushed.body, false)
                    && pushed.slots.last_slot().is_none()
                    && pushed.slots.len() == program.fields.len()
                    && pushed.slots.seen_slice().iter().all(|seen| !seen)
                    && pushed.semantic_slots.len() == program.fields.len()
                    && pushed.semantic_slots.iter().all(Vec::is_empty)
                    && pushed.waiting.is_none()
                    && pushed.par_components == 0
                    && pushed.par_locally_free_range.is_none()
            }
            (
                Self::Map {
                    region,
                    owner,
                    field,
                    value_type_id,
                },
                Frame::Map(pushed),
            ) => {
                region.same_view(&pushed.region)
                    && std::ptr::eq(*owner, pushed.owner)
                    && *field == pushed.outer_field
                    && *value_type_id == pushed.value_type_id
                    && region.position_at(0).map(|position| position.physical)
                        == Some(pushed.region_start)
                    && pushed.cursor.matches_initial_view(&pushed.region, true)
                    && pushed.key.is_none()
                    && pushed.key_position.is_none()
                    && !pushed.saw_value
                    && pushed.value_position.is_none()
                    && pushed.value.is_none()
                    && !pushed.waiting_value
            }
            (
                Self::EPathMap {
                    region,
                    owner,
                    field,
                },
                Frame::EPathMap(pushed),
            ) => {
                region.same_view(&pushed.snapshot)
                    && std::ptr::eq(*owner, pushed.owner)
                    && *field == pushed.field
                    && region.position_at(0).map(|position| position.physical)
                        == Some(pushed.region_start)
                    && pushed.cursor.mode() == pushed.mode
                    && pushed.cursor.stats().value_table_bytes_read == 0
                    && pushed.cursor.stats().value_table_bytes_traversed == 0
                    && pushed.cursor.stats().topology_events == 0
                    && pushed.cursor.stats().value_records == 0
                    && pushed.cursor.stats().terminal_checks == 0
                    && pushed.cursor.terminal_source_image().is_none()
                    && pushed.waiting.is_none()
                    && pushed.waiting_child.is_none()
                    && pushed.path.active_obligation().is_none()
                    && pushed.entries_canonical_path_stable
                    && pushed.last_source
                        == CanonicalPathSource::in_source(
                            pushed.region_start.source,
                            pushed.region_start.offset,
                        )
                    && pushed.terminal_source.is_none()
            }
            (Self::CanonicalPath(region), Frame::CanonicalPath(pushed)) => {
                region.same_view(&pushed.input)
                    && pushed.cursor.bytes_yielded() == 0
                    && pushed.cursor.bytes_traversed() == 0
                    && pushed.reducer.active_obligation().is_none()
                    && pushed.waiting.is_none()
                    && pushed.waiting_child.is_none()
                    && pushed.terminal_source.is_none()
            }
            _ => false,
        }
    }

    fn region(&self) -> &AdmissionRegion<'src> {
        match self {
            Self::Message { region, .. }
            | Self::Map { region, .. }
            | Self::EPathMap { region, .. }
            | Self::CanonicalPath(region) => region,
        }
    }

    fn request_kind(&self) -> FrameRequestKind {
        match self {
            Self::Message { .. } => FrameRequestKind::Message,
            Self::Map { .. } => FrameRequestKind::Map,
            Self::EPathMap { .. } => FrameRequestKind::EPathMap,
            Self::CanonicalPath(_) => FrameRequestKind::CanonicalPath,
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

    fn push_canonical_path(frames: &mut Vec<Frame<'src>>, region: AdmissionRegion<'src>) {
        frames.push(Frame::CanonicalPath(CanonicalPathFrame::new(region)));
    }
}

#[derive(Clone)]
struct MessageSummary<'src> {
    semantic: SemanticSummary<'src>,
}

struct MessageFinishObservation<'src> {
    selected_kind: SemanticKind,
    summary: MessageSummary<'src>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ComponentFinishObservation {
    family: ParComponentTag,
    complete: bool,
    canonical_path_stable: bool,
    score_handle: CanonicalScoreHandle,
}

impl<'src> MessageFinishObservation<'src> {
    fn component(&self) -> Option<ComponentFinishObservation> {
        match &self.summary.semantic {
            SemanticSummary::Component(component) => Some(ComponentFinishObservation {
                family: component.family,
                complete: component.complete,
                canonical_path_stable: component.canonical_path_stable,
                score_handle: component.score,
            }),
            _ => None,
        }
    }

    fn into_action(self) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
        if !self.summary.semantic.matches_kind(self.selected_kind) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "message reducer returned a different semantic summary kind",
            ));
        }
        Ok(FrameAction::Complete(FrameSummary::Message(self.summary)))
    }
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

impl FrameSummary<'_> {
    fn request_kind(&self) -> FrameRequestKind {
        match self {
            Self::Message(_) => FrameRequestKind::Message,
            Self::Map(_) => FrameRequestKind::Map,
            Self::EPathMap(_) => FrameRequestKind::EPathMap,
            Self::CanonicalPath(_) => FrameRequestKind::CanonicalPath,
        }
    }
}

fn same_optional_view<'src>(
    left: &Option<AdmissionRegion<'src>>,
    right: &Option<AdmissionRegion<'src>>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.same_view(right),
        _ => false,
    }
}

fn same_optional_par_summary<'src>(
    source: &Option<ParSummarySource<'src>>,
    value: &Option<ParSummary<'src>>,
) -> bool {
    match (source, value) {
        (None, None) => true,
        (Some(source), Some(value)) => source.matches(value),
        _ => false,
    }
}

/// A constant-size identity for an already-produced Par summary. Its byte
/// regions are shallow indexed views; no identity bytes are copied.
struct ParSummarySource<'src> {
    score: CanonicalScoreHandle,
    semantic_default: bool,
    canonical_path_stable: bool,
    identity_prefix: AdmissionRegion<'src>,
    identity_suffix: AdmissionRegion<'src>,
}

impl<'src> ParSummarySource<'src> {
    fn before(summary: &ParSummary<'src>) -> Self {
        Self {
            score: summary.score,
            semantic_default: summary.semantic_default,
            canonical_path_stable: summary.canonical_path_stable,
            identity_prefix: summary.identity_prefix.clone(),
            identity_suffix: summary.identity_suffix.clone(),
        }
    }

    fn matches(&self, summary: &ParSummary<'src>) -> bool {
        self.score == summary.score
            && self.semantic_default == summary.semantic_default
            && self.canonical_path_stable == summary.canonical_path_stable
            && self.identity_prefix.same_view(&summary.identity_prefix)
            && self.identity_suffix.same_view(&summary.identity_suffix)
    }

    fn matches_source(&self, other: &Self) -> bool {
        self.score == other.score
            && self.semantic_default == other.semantic_default
            && self.canonical_path_stable == other.canonical_path_stable
            && self.identity_prefix.same_view(&other.identity_prefix)
            && self.identity_suffix.same_view(&other.identity_suffix)
    }
}

struct MapSummarySource<'src> {
    key: AdmissionRegion<'src>,
    key_position: CanonicalByteProvenance,
    value: Option<ParSummarySource<'src>>,
}

impl<'src> MapSummarySource<'src> {
    fn before(summary: &MapSummary<'src>) -> Self {
        Self {
            key: summary.key.clone(),
            key_position: summary.key_position,
            value: summary.value.as_ref().map(ParSummarySource::before),
        }
    }

    fn matches(&self, summary: &MapSummary<'src>) -> bool {
        self.key.same_view(&summary.key)
            && self.key_position == summary.key_position
            && match (&self.value, &summary.value) {
                (None, None) => true,
                (Some(source), Some(value)) => source.matches(value),
                _ => false,
            }
    }

    fn matches_source(&self, other: &Self) -> bool {
        self.key.same_view(&other.key)
            && self.key_position == other.key_position
            && match (&self.value, &other.value) {
                (None, None) => true,
                (Some(left), Some(right)) => left.matches_source(right),
                _ => false,
            }
    }
}

struct EPathMapSummarySource<'src> {
    mode: EPathMapMode,
    snapshot: AdmissionRegion<'src>,
    entries_canonical_path_stable: bool,
}

impl<'src> EPathMapSummarySource<'src> {
    fn before(summary: &EPathMapSummary<'src>) -> Self {
        Self {
            mode: summary.mode,
            snapshot: summary.snapshot.clone(),
            entries_canonical_path_stable: summary.entries_canonical_path_stable,
        }
    }

    fn matches(&self, summary: &EPathMapSummary<'src>) -> bool {
        self.mode == summary.mode
            && self.snapshot.same_view(&summary.snapshot)
            && self.entries_canonical_path_stable == summary.entries_canonical_path_stable
    }
}

/// Captures scalar values and indexed-view identity. The only vector case is
/// an owned ConnectiveBody: its allocation identity and shape survive a move.
enum SemanticSummarySource<'src> {
    Opaque,
    Par(ParSummarySource<'src>),
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
    ExprPayload {
        score: Option<CanonicalScoreHandle>,
        stable: bool,
        fixed: bool,
        snapshot: Option<AdmissionRegion<'src>>,
    },
    KeyValuePair {
        key: Option<ParSummarySource<'src>>,
        value: Option<ParSummarySource<'src>>,
    },
    BigRational {
        numerator: AdmissionRegion<'src>,
        denominator: AdmissionRegion<'src>,
    },
    FixedPoint {
        unscaled: AdmissionRegion<'src>,
        scale: u32,
    },
    ConnectiveBody {
        allocation: *const ParSummary<'src>,
        len: usize,
        capacity: usize,
    },
    VarRef {
        index: i32,
        depth: i32,
    },
    Bytes(AdmissionRegion<'src>),
    Unit,
}

impl<'src> SemanticSummarySource<'src> {
    fn before(summary: &SemanticSummary<'src>) -> Self {
        match summary {
            SemanticSummary::Opaque => Self::Opaque,
            SemanticSummary::Par(value) => Self::Par(ParSummarySource::before(value)),
            SemanticSummary::Component(value) => Self::Component(value.clone()),
            SemanticSummary::Var(value) => Self::Var(*value),
            SemanticSummary::ReceiveBind {
                score,
                source_present,
            } => Self::ReceiveBind {
                score: *score,
                source_present: *source_present,
            },
            SemanticSummary::CostScore(value) => Self::CostScore(*value),
            SemanticSummary::MatchCase {
                score,
                required_present,
                canonical_fixed_point,
            } => Self::MatchCase {
                score: *score,
                required_present: *required_present,
                canonical_fixed_point: *canonical_fixed_point,
            },
            SemanticSummary::ExprPayload(value) => Self::ExprPayload {
                score: value.score,
                stable: value.canonical_path_stable,
                fixed: value.canonical_fixed_point,
                snapshot: value.pathmap_snapshot.clone(),
            },
            SemanticSummary::KeyValuePair { key, value } => Self::KeyValuePair {
                key: key.as_ref().map(ParSummarySource::before),
                value: value.as_ref().map(ParSummarySource::before),
            },
            SemanticSummary::BigRational {
                numerator,
                denominator,
            } => Self::BigRational {
                numerator: numerator.clone(),
                denominator: denominator.clone(),
            },
            SemanticSummary::FixedPoint { unscaled, scale } => Self::FixedPoint {
                unscaled: unscaled.clone(),
                scale: *scale,
            },
            SemanticSummary::ConnectiveBody(values) => Self::ConnectiveBody {
                allocation: values.as_ptr(),
                len: values.len(),
                capacity: values.capacity(),
            },
            SemanticSummary::VarRef { index, depth } => Self::VarRef {
                index: *index,
                depth: *depth,
            },
            SemanticSummary::Bytes(value) => Self::Bytes(value.clone()),
            SemanticSummary::Unit => Self::Unit,
        }
    }

    fn matches(&self, summary: &SemanticSummary<'src>) -> bool {
        match (self, summary) {
            (Self::Opaque, SemanticSummary::Opaque) | (Self::Unit, SemanticSummary::Unit) => true,
            (Self::Par(source), SemanticSummary::Par(value)) => source.matches(value),
            (Self::Component(source), SemanticSummary::Component(value)) => {
                source.family == value.family
                    && source.score == value.score
                    && source.complete == value.complete
                    && source.canonical_path_stable == value.canonical_path_stable
            }
            (Self::Var(source), SemanticSummary::Var(value))
            | (Self::CostScore(source), SemanticSummary::CostScore(value)) => source == value,
            (
                Self::ReceiveBind {
                    score,
                    source_present,
                },
                SemanticSummary::ReceiveBind {
                    score: value_score,
                    source_present: value_present,
                },
            ) => score == value_score && source_present == value_present,
            (
                Self::MatchCase {
                    score,
                    required_present,
                    canonical_fixed_point,
                },
                SemanticSummary::MatchCase {
                    score: value_score,
                    required_present: value_required,
                    canonical_fixed_point: value_fixed,
                },
            ) => {
                score == value_score
                    && required_present == value_required
                    && canonical_fixed_point == value_fixed
            }
            (
                Self::ExprPayload {
                    score,
                    stable,
                    fixed,
                    snapshot,
                },
                SemanticSummary::ExprPayload(value),
            ) => {
                score == &value.score
                    && stable == &value.canonical_path_stable
                    && fixed == &value.canonical_fixed_point
                    && same_optional_view(snapshot, &value.pathmap_snapshot)
            }
            (
                Self::KeyValuePair { key, value },
                SemanticSummary::KeyValuePair {
                    key: value_key,
                    value: value_value,
                },
            ) => {
                same_optional_par_summary(key, value_key)
                    && same_optional_par_summary(value, value_value)
            }
            (
                Self::BigRational {
                    numerator,
                    denominator,
                },
                SemanticSummary::BigRational {
                    numerator: value_num,
                    denominator: value_den,
                },
            ) => numerator.same_view(value_num) && denominator.same_view(value_den),
            (
                Self::FixedPoint { unscaled, scale },
                SemanticSummary::FixedPoint {
                    unscaled: value,
                    scale: value_scale,
                },
            ) => unscaled.same_view(value) && scale == value_scale,
            (
                Self::ConnectiveBody {
                    allocation,
                    len,
                    capacity,
                },
                SemanticSummary::ConnectiveBody(values),
            ) => {
                *allocation == values.as_ptr()
                    && *len == values.len()
                    && *capacity == values.capacity()
            }
            (
                Self::VarRef { index, depth },
                SemanticSummary::VarRef {
                    index: value_index,
                    depth: value_depth,
                },
            ) => index == value_index && depth == value_depth,
            (Self::Bytes(source), SemanticSummary::Bytes(value)) => source.same_view(value),
            _ => false,
        }
    }
}

enum FrameSummarySource<'src> {
    Message(SemanticSummarySource<'src>),
    Map(MapSummarySource<'src>),
    EPathMap(EPathMapSummarySource<'src>),
    CanonicalPath(
        #[expect(
            dead_code,
            reason = "before() copies the canonical-path summary here, but every reader of a frame summary source matches only its Message, Map or EPathMap variants"
        )]
        CanonicalPathSummary,
    ),
}

/// The accepted value or resolution actually passed through a parent frame.
/// This is constant-size and carries only shallow indexed source views.
enum FrameAcceptEffect<'src> {
    Stored,
    EPathMapValue(ParSummarySource<'src>),
    ResolvedObligation {
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    },
}

impl<'src> FrameSummarySource<'src> {
    fn before(summary: &FrameSummary<'src>) -> Self {
        match summary {
            FrameSummary::Message(value) => {
                Self::Message(SemanticSummarySource::before(&value.semantic))
            }
            FrameSummary::Map(value) => Self::Map(MapSummarySource::before(value)),
            FrameSummary::EPathMap(value) => Self::EPathMap(EPathMapSummarySource::before(value)),
            FrameSummary::CanonicalPath(value) => Self::CanonicalPath(*value),
        }
    }

    fn matches_stored(&self, stored: &SemanticValue<'src>) -> bool {
        match (self, stored) {
            (Self::Message(source), SemanticValue::Message(value)) => {
                source.matches(&value.semantic)
            }
            (Self::Map(source), SemanticValue::Map(value)) => source.matches(value),
            (Self::EPathMap(source), SemanticValue::EPathMap(value)) => source.matches(value),
            _ => false,
        }
    }
}

enum FrameAction<'src> {
    Continue,
    Push(FrameRequest<'src>),
    Complete(FrameSummary<'src>),
}

enum MapStepSource<'src> {
    Field(MapFieldObservation<'src>),
    Eof(MapEofObservation<'src>),
}

// Property tests retain the historical value-shaped observation API. The
// production driver never constructs this enum.
#[cfg(test)]
enum FrameStep<'src> {
    MessageContinue(
        #[expect(
            dead_code,
            reason = "take_test_step moves the checked field receipt into this variant, but no property test reads a message-continue receipt"
        )]
        MessageFieldSourceReceipt<'src>,
    ),
    MessagePush {
        request: FrameRequest<'src>,
        field: MessageFieldSourceReceipt<'src>,
    },
    MessageComplete {
        summary: FrameSummary<'src>,
        eof: MessageEofSourceReceipt<'src>,
    },
    Other {
        action: FrameAction<'src>,
        map_source: Option<MapStepSource<'src>>,
    },
}

struct DriverStepSource<'src> {
    stack_len: usize,
    top_kind: FrameRequestKind,
    top_program: Option<&'static RhoMatchProgram>,
    map_cursor: Option<IndexedCanonicalFieldCursorStats>,
    map_eof_before: Option<MapEofObservation<'src>>,
    score_entries: usize,
}

/// Constant-size source facts for the parent suspended below a completing
/// child. Indexed regions are cloned as view handles, never as payload bytes.
enum EPathMapReturnSource<'src> {
    MapValue {
        returned: ParSummarySource<'src>,
        entries_before: bool,
        active_before: Option<CanonicalPathObligationProgress>,
    },
    Obligation {
        id: CanonicalPathObligationId,
        kind: CanonicalPathObligationKind,
        resolution: CanonicalPathObligationResolution,
        returned_par: Option<ParSummarySource<'src>>,
    },
}

enum CompleteParentSource<'src> {
    Message {
        program: &'static RhoMatchProgram,
        body: AdmissionRegion<'src>,
        cursor: IndexedCanonicalFieldCursorStats,
        last_slot: Option<usize>,
        slot: usize,
        occurrences: usize,
        seen_allocation: *const bool,
        seen_len: usize,
        slots_allocation: *const Vec<SemanticOccurrence<'src>>,
        slots_len: usize,
        returned: FrameSummarySource<'src>,
    },
    Map {
        region: AdmissionRegion<'src>,
        key: Option<AdmissionRegion<'src>>,
        cursor: IndexedCanonicalFieldCursorStats,
        returned: ParSummarySource<'src>,
    },
    EPathMap {
        snapshot: AdmissionRegion<'src>,
        cursor: IndexedCanonicalEpm1TopologyStats,
        returned: EPathMapReturnSource<'src>,
    },
    CanonicalPath {
        input: AdmissionRegion<'src>,
        child: AdmissionRegion<'src>,
        bytes_yielded: usize,
        bytes_traversed: usize,
        obligation: PathObligationWait,
        resolution: CanonicalPathObligationResolution,
        returned_par: Option<ParSummarySource<'src>>,
    },
}

impl<'src> CompleteParentSource<'src> {
    fn matches_effect(&self, effect: &FrameAcceptEffect<'src>) -> bool {
        match (self, effect) {
            (Self::Message { .. } | Self::Map { .. }, FrameAcceptEffect::Stored) => true,
            (
                Self::EPathMap {
                    returned: EPathMapReturnSource::MapValue { returned, .. },
                    ..
                },
                FrameAcceptEffect::EPathMapValue(value),
            ) => returned.matches_source(value),
            (
                Self::EPathMap {
                    returned:
                        EPathMapReturnSource::Obligation {
                            id,
                            resolution,
                            returned_par,
                            ..
                        },
                    ..
                },
                FrameAcceptEffect::ResolvedObligation {
                    id: applied_id,
                    resolution: applied_resolution,
                },
            ) => {
                id == applied_id
                    && resolution == applied_resolution
                    && match (resolution, returned_par) {
                        (
                            CanonicalPathObligationResolution::RawPar {
                                canonical_path_stable,
                            },
                            Some(returned),
                        ) => returned.canonical_path_stable == *canonical_path_stable,
                        (CanonicalPathObligationResolution::NestedEpm1 { .. }, None) => true,
                        _ => false,
                    }
            }
            (
                Self::CanonicalPath {
                    obligation,
                    resolution,
                    returned_par,
                    ..
                },
                FrameAcceptEffect::ResolvedObligation {
                    id: applied_id,
                    resolution: applied_resolution,
                },
            ) => {
                obligation.id == *applied_id
                    && resolution == applied_resolution
                    && match (resolution, returned_par) {
                        (
                            CanonicalPathObligationResolution::RawPar {
                                canonical_path_stable,
                            },
                            Some(returned),
                        ) => returned.canonical_path_stable == *canonical_path_stable,
                        (CanonicalPathObligationResolution::NestedEpm1 { .. }, None) => true,
                        _ => false,
                    }
            }
            _ => false,
        }
    }

    fn resolution_delta(&self) -> Option<usize> {
        match self {
            Self::EPathMap {
                returned:
                    EPathMapReturnSource::Obligation {
                        kind, resolution, ..
                    },
                ..
            }
            | Self::CanonicalPath {
                obligation: PathObligationWait { kind, .. },
                resolution,
                ..
            } => match (kind, resolution) {
                (
                    CanonicalPathObligationKind::RawParEscape,
                    CanonicalPathObligationResolution::RawPar { .. },
                )
                | (
                    CanonicalPathObligationKind::NestedEpm1,
                    CanonicalPathObligationResolution::NestedEpm1 { .. },
                ) => Some(1),
                _ => None,
            },
            _ => Some(0),
        }
    }

    fn before(
        frame: &Frame<'src>,
        child: &Frame<'src>,
        summary: &FrameSummary<'src>,
    ) -> Option<Self> {
        match frame {
            Frame::Message(parent) => {
                let (slot, expected) = match parent.waiting.as_ref()? {
                    MessageWait::Child { slot, .. } => (*slot, FrameRequestKind::Message),
                    MessageWait::Map { slot, .. } => (*slot, FrameRequestKind::Map),
                    MessageWait::EPathMap { slot, .. } => (*slot, FrameRequestKind::EPathMap),
                };
                (summary.request_kind() == expected).then(|| Self::Message {
                    program: parent.program,
                    body: parent.body.clone(),
                    cursor: parent.cursor.stats(),
                    last_slot: parent.slots.last_slot(),
                    slot,
                    occurrences: parent.semantic_slots[slot].len(),
                    seen_allocation: parent.slots.seen_slice().as_ptr(),
                    seen_len: parent.slots.len(),
                    slots_allocation: parent.semantic_slots.as_ptr(),
                    slots_len: parent.semantic_slots.len(),
                    returned: FrameSummarySource::before(summary),
                })
            }
            Frame::Map(parent) => {
                let FrameSummary::Message(MessageSummary {
                    semantic: SemanticSummary::Par(returned),
                }) = summary
                else {
                    return None;
                };
                parent.waiting_value.then(|| Self::Map {
                    region: parent.region.clone(),
                    key: parent.key.clone(),
                    cursor: parent.cursor.stats(),
                    returned: ParSummarySource::before(returned),
                })
            }
            Frame::EPathMap(parent) => {
                let retained = parent.waiting_child.as_ref()?;
                let returned = match parent.waiting.as_ref()? {
                    EPathMapWait::MapValue => {
                        let Frame::Message(child) = child else {
                            return None;
                        };
                        if !retained.same_view(&child.body) {
                            return None;
                        }
                        let FrameSummary::Message(MessageSummary {
                            semantic: SemanticSummary::Par(par),
                        }) = summary
                        else {
                            return None;
                        };
                        EPathMapReturnSource::MapValue {
                            returned: ParSummarySource::before(par),
                            entries_before: parent.entries_canonical_path_stable,
                            active_before: parent.path.active_obligation(),
                        }
                    }
                    EPathMapWait::Obligation(waiting) => {
                        let child_region = match (waiting.kind, child) {
                            (CanonicalPathObligationKind::RawParEscape, Frame::Message(child)) => {
                                &child.body
                            }
                            (CanonicalPathObligationKind::NestedEpm1, Frame::EPathMap(child)) => {
                                &child.snapshot
                            }
                            _ => return None,
                        };
                        if !retained.same_view(child_region) {
                            return None;
                        }
                        let resolution = waiting.resolution(summary)?;
                        let returned_par = match (resolution, summary) {
                            (
                                CanonicalPathObligationResolution::RawPar { .. },
                                FrameSummary::Message(MessageSummary {
                                    semantic: SemanticSummary::Par(par),
                                }),
                            ) => Some(ParSummarySource::before(par)),
                            (CanonicalPathObligationResolution::NestedEpm1 { .. }, _) => None,
                            _ => return None,
                        };
                        EPathMapReturnSource::Obligation {
                            id: waiting.id,
                            kind: waiting.kind,
                            resolution,
                            returned_par,
                        }
                    }
                };
                Some(Self::EPathMap {
                    snapshot: parent.snapshot.clone(),
                    cursor: parent.cursor.stats(),
                    returned,
                })
            }
            Frame::CanonicalPath(parent) => {
                let waiting = parent.waiting.as_ref()?;
                let retained = parent.waiting_child.as_ref()?;
                let child_region = match (waiting.kind, child) {
                    (CanonicalPathObligationKind::RawParEscape, Frame::Message(child)) => {
                        &child.body
                    }
                    (CanonicalPathObligationKind::NestedEpm1, Frame::EPathMap(child)) => {
                        &child.snapshot
                    }
                    _ => return None,
                };
                if !retained.same_view(child_region) {
                    return None;
                }
                let resolution = waiting.resolution(summary)?;
                let returned_par = match (resolution, summary) {
                    (
                        CanonicalPathObligationResolution::RawPar { .. },
                        FrameSummary::Message(MessageSummary {
                            semantic: SemanticSummary::Par(par),
                        }),
                    ) => Some(ParSummarySource::before(par)),
                    (CanonicalPathObligationResolution::NestedEpm1 { .. }, _) => None,
                    _ => return None,
                };
                Some(Self::CanonicalPath {
                    input: parent.input.clone(),
                    child: retained.clone(),
                    bytes_yielded: parent.cursor.bytes_yielded(),
                    bytes_traversed: parent.cursor.bytes_traversed(),
                    obligation: PathObligationWait {
                        id: waiting.id,
                        kind: waiting.kind,
                    },
                    resolution,
                    returned_par,
                })
            }
        }
    }

    fn before_accept(&self, frame: &Frame<'src>) -> bool {
        match (self, frame) {
            (
                Self::Message {
                    program,
                    body,
                    cursor,
                    last_slot,
                    slot,
                    occurrences,
                    seen_allocation,
                    seen_len,
                    slots_allocation,
                    slots_len,
                    ..
                },
                Frame::Message(parent),
            ) => {
                std::ptr::eq(*program, parent.program)
                    && body.same_view(&parent.body)
                    && *cursor == parent.cursor.stats()
                    && *last_slot == parent.slots.last_slot()
                    && parent.waiting.is_some()
                    && *seen_allocation == parent.slots.seen_slice().as_ptr()
                    && *seen_len == parent.slots.len()
                    && *slots_allocation == parent.semantic_slots.as_ptr()
                    && *slots_len == parent.semantic_slots.len()
                    && parent.semantic_slots[*slot].len() == *occurrences
            }
            (
                Self::Map {
                    region,
                    key,
                    cursor,
                    ..
                },
                Frame::Map(parent),
            ) => {
                region.same_view(&parent.region)
                    && *cursor == parent.cursor.stats()
                    && match (key.as_ref(), parent.key.as_ref()) {
                        (None, None) => true,
                        (Some(left), Some(right)) => left.same_view(right),
                        _ => false,
                    }
                    && parent.waiting_value
                    && parent.value.is_none()
            }
            (
                Self::EPathMap {
                    snapshot,
                    cursor,
                    returned,
                },
                Frame::EPathMap(parent),
            ) => {
                snapshot.same_view(&parent.snapshot)
                    && *cursor == parent.cursor.stats()
                    && parent.waiting_child.is_some()
                    && match (returned, parent.waiting.as_ref()) {
                        (
                            EPathMapReturnSource::MapValue {
                                entries_before,
                                active_before,
                                ..
                            },
                            Some(EPathMapWait::MapValue),
                        ) => {
                            *entries_before == parent.entries_canonical_path_stable
                                && *active_before == parent.path.active_obligation()
                        }
                        (
                            EPathMapReturnSource::Obligation { id, kind, .. },
                            Some(EPathMapWait::Obligation(waiting)),
                        ) => {
                            waiting.id == *id
                                && waiting.kind == *kind
                                && parent.path.active_obligation().is_some_and(|active| {
                                    active.obligation.id == *id
                                        && active.obligation.kind == *kind
                                        && active.awaiting_resolution
                                })
                        }
                        _ => false,
                    }
            }
            (
                Self::CanonicalPath {
                    input,
                    child,
                    bytes_yielded,
                    bytes_traversed,
                    obligation,
                    ..
                },
                Frame::CanonicalPath(parent),
            ) => {
                input.same_view(&parent.input)
                    && parent
                        .waiting_child
                        .as_ref()
                        .is_some_and(|view| child.same_view(view))
                    && *bytes_yielded == parent.cursor.bytes_yielded()
                    && *bytes_traversed == parent.cursor.bytes_traversed()
                    && parent.waiting.as_ref().is_some_and(|waiting| {
                        waiting.id == obligation.id && waiting.kind == obligation.kind
                    })
                    && parent.reducer.active_obligation().is_some_and(|active| {
                        active.obligation.id == obligation.id
                            && active.obligation.kind == obligation.kind
                            && active.awaiting_resolution
                    })
            }
            _ => false,
        }
    }

    fn after_accept(&self, frame: &Frame<'src>) -> bool {
        match (self, frame) {
            (
                Self::Message {
                    program,
                    body,
                    cursor,
                    last_slot,
                    slot,
                    occurrences,
                    seen_allocation,
                    seen_len,
                    slots_allocation,
                    slots_len,
                    returned,
                },
                Frame::Message(parent),
            ) => {
                std::ptr::eq(*program, parent.program)
                    && body.same_view(&parent.body)
                    && *cursor == parent.cursor.stats()
                    && *last_slot == parent.slots.last_slot()
                    && parent.waiting.is_none()
                    && *seen_allocation == parent.slots.seen_slice().as_ptr()
                    && *seen_len == parent.slots.len()
                    && *slots_allocation == parent.semantic_slots.as_ptr()
                    && *slots_len == parent.semantic_slots.len()
                    && occurrences.checked_add(1) == Some(parent.semantic_slots[*slot].len())
                    && parent.semantic_slots[*slot]
                        .last()
                        .is_some_and(|occurrence| returned.matches_stored(&occurrence.value))
            }
            (
                Self::Map {
                    region,
                    key,
                    cursor,
                    returned,
                },
                Frame::Map(parent),
            ) => {
                region.same_view(&parent.region)
                    && *cursor == parent.cursor.stats()
                    && match (key.as_ref(), parent.key.as_ref()) {
                        (None, None) => true,
                        (Some(left), Some(right)) => left.same_view(right),
                        _ => false,
                    }
                    && !parent.waiting_value
                    && parent
                        .value
                        .as_ref()
                        .is_some_and(|value| returned.matches(value))
            }
            (
                Self::EPathMap {
                    snapshot,
                    cursor,
                    returned,
                },
                Frame::EPathMap(parent),
            ) => {
                snapshot.same_view(&parent.snapshot)
                    && *cursor == parent.cursor.stats()
                    && parent.waiting.is_none()
                    && parent.waiting_child.is_none()
                    && match returned {
                        EPathMapReturnSource::MapValue {
                            returned,
                            entries_before,
                            active_before,
                        } => {
                            parent.entries_canonical_path_stable
                                == (*entries_before && returned.canonical_path_stable)
                                && parent.path.active_obligation() == *active_before
                        }
                        EPathMapReturnSource::Obligation { id, .. } => {
                            parent.path.active_obligation().is_none_or(|active| {
                                active.obligation.id != *id || !active.awaiting_resolution
                            })
                        }
                    }
            }
            (
                Self::CanonicalPath {
                    input,
                    bytes_yielded,
                    bytes_traversed,
                    obligation,
                    ..
                },
                Frame::CanonicalPath(parent),
            ) => {
                input.same_view(&parent.input)
                    && *bytes_yielded == parent.cursor.bytes_yielded()
                    && *bytes_traversed == parent.cursor.bytes_traversed()
                    && parent.waiting.is_none()
                    && parent.waiting_child.is_none()
                    && parent.reducer.active_obligation().is_none_or(|active| {
                        active.obligation.id != obligation.id || !active.awaiting_resolution
                    })
            }
            _ => false,
        }
    }
}

struct DriverCompleteSource<'src> {
    stack_len_before: usize,
    completed_kind: FrameRequestKind,
    completed_program: Option<&'static RhoMatchProgram>,
    score_len_before: usize,
    score_len_after_finish: usize,
    resolutions_before_accept: usize,
    resolution_delta: usize,
    parent: Option<CompleteParentSource<'src>>,
    terminal: Option<Box<CompleteTerminalSource<'src>>>,
}

/// A bounded source image retained across the pop. The indexed regions below
/// are cloned view handles; neither variant copies input or records a trace.
enum CompleteTerminalSource<'src> {
    EPathMap {
        snapshot: AdmissionRegion<'src>,
        region_start: CanonicalByteProvenance,
        end: CanonicalByteProvenance,
        cursor: IndexedCanonicalEpm1TopologyStats,
        cursor_source: IndexedCanonicalEpm1TerminalSourceImage,
        mode: EPathMapMode,
        entries_canonical_path_stable: bool,
        protobuf_read_before: usize,
        protobuf_traversed_before: usize,
        events_before: usize,
        returned: EPathMapSummarySource<'src>,
    },
    CanonicalPath {
        input: AdmissionRegion<'src>,
        end: CanonicalByteProvenance,
        bytes_yielded: usize,
        bytes_traversed: usize,
        bytes_read_before: usize,
        bytes_traversed_before: usize,
        returned: CanonicalPathSummary,
    },
}

impl<'src> CompleteTerminalSource<'src> {
    fn matches_final_stats(&self, stats: &CanonicalProtobufAdmissionStats) -> bool {
        match self {
            Self::EPathMap {
                snapshot,
                region_start,
                end,
                cursor,
                cursor_source,
                mode,
                entries_canonical_path_stable,
                protobuf_read_before,
                protobuf_traversed_before,
                events_before,
                returned,
            } => {
                snapshot.position_at(0).map(|position| position.physical) == Some(*region_start)
                    && snapshot.end_position().physical == *end
                    && cursor_source.mode == *mode
                    && cursor_source.snapshot_len == snapshot.len()
                    && cursor_source.value_table_offset == snapshot.len()
                    && cursor_source.event_index == cursor.topology_events
                    && cursor_source.map_values_seen == cursor.value_records
                    && cursor_source.terminal_checks == cursor.terminal_checks
                    && cursor_source.terminal_checks == 1
                    && returned.snapshot.same_view(snapshot)
                    && returned.mode == *mode
                    && returned.entries_canonical_path_stable == *entries_canonical_path_stable
                    && protobuf_read_before
                        .checked_add(cursor.prefix_bytes_read)
                        .and_then(|count| count.checked_add(cursor.value_table_bytes_read))
                        == Some(stats.protobuf_bytes_read)
                    && protobuf_traversed_before
                        .checked_add(cursor.prefix_bytes_traversed)
                        .and_then(|count| count.checked_add(cursor.value_table_bytes_traversed))
                        == Some(stats.protobuf_bytes_traversed)
                    && events_before.checked_add(cursor.topology_events)
                        == Some(stats.epathmap_events)
            }
            Self::CanonicalPath {
                input,
                end,
                bytes_yielded,
                bytes_traversed,
                bytes_read_before,
                bytes_traversed_before,
                ..
            } => {
                input.end_position().physical == *end
                    && *bytes_yielded == input.len()
                    && *bytes_traversed <= *bytes_yielded
                    && bytes_read_before.checked_add(*bytes_yielded)
                        == Some(stats.canonical_path_bytes_read)
                    && bytes_traversed_before.checked_add(*bytes_traversed)
                        == Some(stats.canonical_path_bytes_traversed)
            }
        }
    }

    fn matches_root(&self, summary: &FrameSummary<'src>) -> bool {
        match (self, summary) {
            (Self::EPathMap { returned, .. }, FrameSummary::EPathMap(value)) => {
                returned.matches(value)
            }
            (Self::CanonicalPath { returned, .. }, FrameSummary::CanonicalPath(value)) => {
                returned == value
            }
            _ => false,
        }
    }

    fn matches_accepted_parent(
        &self,
        parent: &CompleteParentSource<'src>,
        effect: &FrameAcceptEffect<'src>,
    ) -> bool {
        let Self::EPathMap {
            snapshot,
            mode,
            entries_canonical_path_stable,
            returned,
            ..
        } = self
        else {
            return false;
        };
        let nested_mode = match mode {
            EPathMapMode::Empty => NestedEpmMode::Empty,
            EPathMapMode::Set => NestedEpmMode::Set,
            EPathMapMode::Map => NestedEpmMode::Map,
        };
        match (parent, effect) {
            (
                CompleteParentSource::Message {
                    returned: FrameSummarySource::EPathMap(parent_returned),
                    ..
                },
                FrameAcceptEffect::Stored,
            ) => {
                parent_returned.snapshot.same_view(snapshot)
                    && parent_returned.mode == returned.mode
                    && parent_returned.entries_canonical_path_stable
                        == returned.entries_canonical_path_stable
            }
            (
                CompleteParentSource::EPathMap {
                    returned:
                        EPathMapReturnSource::Obligation {
                            kind: CanonicalPathObligationKind::NestedEpm1,
                            resolution:
                                CanonicalPathObligationResolution::NestedEpm1 {
                                    mode: applied_mode,
                                    entries_stable,
                                },
                            ..
                        },
                    ..
                },
                FrameAcceptEffect::ResolvedObligation { .. },
            ) => applied_mode == &nested_mode && entries_stable == entries_canonical_path_stable,
            (
                CompleteParentSource::CanonicalPath {
                    child,
                    resolution:
                        CanonicalPathObligationResolution::NestedEpm1 {
                            mode: applied_mode,
                            entries_stable,
                        },
                    ..
                },
                FrameAcceptEffect::ResolvedObligation { .. },
            ) => {
                child.same_view(snapshot)
                    && applied_mode == &nested_mode
                    && entries_stable == entries_canonical_path_stable
            }
            _ => false,
        }
    }
}

impl<'src> DriverCompleteSource<'src> {
    fn before_pop(
        driver: &DriverStepSource<'src>,
        frames: &[Frame<'src>],
        summary: &FrameSummary<'src>,
        scores: &CanonicalScoreArena<'src>,
        stats: &CanonicalProtobufAdmissionStats,
    ) -> Option<Self> {
        let child = frames.last()?;
        if driver.stack_len != frames.len()
            || !driver.matches_top(child)
            || child.request_kind() != summary.request_kind()
            || driver.score_entries > scores.source_len()
        {
            return None;
        }
        if let Frame::EPathMap(frame) = child {
            if !frame
                .terminal_source
                .as_ref()?
                .matches_complete(frame, summary, stats)
            {
                return None;
            }
        }
        if let Frame::CanonicalPath(frame) = child {
            if !frame
                .terminal_source
                .as_ref()?
                .matches_complete(frame, summary, stats)
            {
                return None;
            }
        }
        let terminal = match (child, summary) {
            (Frame::EPathMap(frame), FrameSummary::EPathMap(value)) => {
                let source = frame.terminal_source.as_ref()?;
                Some(Box::new(CompleteTerminalSource::EPathMap {
                    snapshot: source.snapshot.clone(),
                    region_start: source.region_start,
                    end: source.end,
                    cursor: source.cursor,
                    cursor_source: source.cursor_source,
                    mode: source.mode,
                    entries_canonical_path_stable: source.entries_canonical_path_stable,
                    protobuf_read_before: source.protobuf_read_before,
                    protobuf_traversed_before: source.protobuf_traversed_before,
                    events_before: source.events_before,
                    returned: EPathMapSummarySource::before(value),
                }))
            }
            (Frame::CanonicalPath(frame), FrameSummary::CanonicalPath(value)) => {
                let source = frame.terminal_source.as_ref()?;
                Some(Box::new(CompleteTerminalSource::CanonicalPath {
                    input: source.input.clone(),
                    end: source.end,
                    bytes_yielded: source.bytes_yielded,
                    bytes_traversed: source.bytes_traversed,
                    bytes_read_before: source.bytes_read_before,
                    bytes_traversed_before: source.bytes_traversed_before,
                    returned: *value,
                }))
            }
            (Frame::EPathMap(_) | Frame::CanonicalPath(_), _) => return None,
            _ => None,
        };
        let parent = if frames.len() > 1 {
            Some(CompleteParentSource::before(
                &frames[frames.len() - 2],
                child,
                summary,
            )?)
        } else {
            None
        };
        let resolution_delta = parent
            .as_ref()
            .map_or(Some(0), CompleteParentSource::resolution_delta)?;
        Some(Self {
            stack_len_before: frames.len(),
            completed_kind: driver.top_kind,
            completed_program: driver.top_program,
            score_len_before: driver.score_entries,
            score_len_after_finish: scores.source_len(),
            resolutions_before_accept: stats.canonical_path_resolutions,
            resolution_delta,
            parent,
            terminal,
        })
    }

    fn matches_root_return(
        &self,
        frames: &[Frame<'src>],
        summary: &FrameSummary<'src>,
        scores: &CanonicalScoreArena<'src>,
        stats: &CanonicalProtobufAdmissionStats,
    ) -> bool {
        self.parent.is_none()
            && self.matches_terminal_root(summary, stats)
            && self.matches_accepted(frames, scores, stats, None)
    }

    fn matches_terminal_root(
        &self,
        summary: &FrameSummary<'src>,
        stats: &CanonicalProtobufAdmissionStats,
    ) -> bool {
        match (&self.terminal, self.completed_kind) {
            (Some(terminal), FrameRequestKind::EPathMap | FrameRequestKind::CanonicalPath) => {
                terminal.matches_final_stats(stats) && terminal.matches_root(summary)
            }
            (None, FrameRequestKind::Message | FrameRequestKind::Map) => true,
            _ => false,
        }
    }

    fn matches_terminal_accept(
        &self,
        stats: &CanonicalProtobufAdmissionStats,
        effect: Option<&FrameAcceptEffect<'src>>,
    ) -> bool {
        match (
            &self.terminal,
            self.completed_kind,
            self.parent.as_ref(),
            effect,
        ) {
            (Some(terminal), FrameRequestKind::EPathMap, Some(parent), Some(effect)) => {
                terminal.matches_final_stats(stats)
                    && terminal.matches_accepted_parent(parent, effect)
            }
            (None, FrameRequestKind::Message | FrameRequestKind::Map, Some(_), Some(_)) => true,
            _ => false,
        }
    }

    fn matches_popped(
        &self,
        frames: &[Frame<'src>],
        scores: &CanonicalScoreArena<'src>,
        stats: &CanonicalProtobufAdmissionStats,
    ) -> bool {
        self.stack_len_before.checked_sub(1) == Some(frames.len())
            && self.score_len_after_finish == scores.source_len()
            && self.resolutions_before_accept == stats.canonical_path_resolutions
            && match (&self.parent, frames.last()) {
                (None, None) => true,
                (Some(source), Some(parent)) => source.before_accept(parent),
                _ => false,
            }
    }

    fn matches_accepted(
        &self,
        frames: &[Frame<'src>],
        scores: &CanonicalScoreArena<'src>,
        stats: &CanonicalProtobufAdmissionStats,
        effect: Option<&FrameAcceptEffect<'src>>,
    ) -> bool {
        self.stack_len_before.checked_sub(1) == Some(frames.len())
            && self.score_len_after_finish == scores.source_len()
            && self
                .resolutions_before_accept
                .checked_add(self.resolution_delta)
                == Some(stats.canonical_path_resolutions)
            && (self.parent.is_none() || self.matches_terminal_accept(stats, effect))
            && match (&self.parent, frames.last()) {
                (None, None) => effect.is_none(),
                (Some(source), Some(parent)) => {
                    effect.is_some_and(|effect| source.matches_effect(effect))
                        && source.after_accept(parent)
                }
                _ => false,
            }
    }
}

impl<'src> DriverStepSource<'src> {
    fn matches_top(&self, frame: &Frame<'_>) -> bool {
        self.top_kind == frame.request_kind()
            && match (self.top_program, frame) {
                (Some(before), Frame::Message(after)) => std::ptr::eq(before, after.program),
                (None, Frame::Message(_)) | (Some(_), _) => false,
                (None, _) => true,
            }
    }

    fn before(frames: &[Frame<'src>], scores: &CanonicalScoreArena<'src>) -> Self {
        let top = frames
            .last()
            .expect("canonical protobuf admission retains its root frame");
        Self {
            stack_len: frames.len(),
            top_kind: top.request_kind(),
            top_program: match top {
                Frame::Message(frame) => Some(frame.program),
                _ => None,
            },
            map_cursor: match top {
                Frame::Map(frame) => Some(frame.cursor.stats()),
                _ => None,
            },
            map_eof_before: match top {
                Frame::Map(frame)
                    if frame.cursor.is_empty()
                        && !frame.waiting_value
                        && frame.cursor.is_terminal_error_free() =>
                {
                    Some(MapEofObservation::from_frame(frame))
                }
                _ => None,
            },
            score_entries: scores.source_len(),
        }
    }

    fn matches_continue(&self, frames: &[Frame<'_>], scores: &CanonicalScoreArena<'_>) -> bool {
        let Some(top) = frames.last() else {
            return false;
        };
        self.stack_len == frames.len()
            && self.score_entries == scores.source_len()
            && self.matches_top(top)
    }

    fn matches_push_parent(&self, frames: &[Frame<'_>], scores: &CanonicalScoreArena<'_>) -> bool {
        let Some(parent_index) = self.stack_len.checked_sub(1) else {
            return false;
        };
        let Some(parent) = frames.get(parent_index) else {
            return false;
        };
        self.stack_len.checked_add(1) == Some(frames.len())
            && self.score_entries == scores.source_len()
            && self.matches_top(parent)
    }
}

#[derive(Clone, Copy)]
struct MessageFieldPrestateWitness {
    cursor: IndexedCanonicalFieldCursorStats,
    last_slot: Option<usize>,
    seen: bool,
}

struct MessageFieldPreStepImage<'src> {
    program: Option<&'static RhoMatchProgram>,
    body: Option<AdmissionRegion<'src>>,
    cursor: IndexedCanonicalFieldCursorStats,
    cursor_source: Option<IndexedMessagePreReadSource>,
    last_slot: Option<usize>,
    waiting: bool,
    par_components: u64,
    par_locally_free_range: Option<CanonicalByteRange>,
    seen: Vec<bool>,
    slot_lengths: Vec<usize>,
}

impl<'src> MessageFieldPreStepImage<'src> {
    fn new() -> Self {
        Self {
            program: None,
            body: None,
            cursor: IndexedCanonicalFieldCursorStats::default(),
            cursor_source: None,
            last_slot: None,
            waiting: false,
            par_components: 0,
            par_locally_free_range: None,
            seen: Vec::new(),
            slot_lengths: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.program = None;
        self.body = None;
        self.cursor_source = None;
        self.seen.clear();
        self.slot_lengths.clear();
    }

    fn capture(&mut self, frame: &Frame<'src>) {
        self.clear();
        if let Frame::Message(frame) = frame {
            self.program = Some(frame.program);
            self.body = Some(frame.body.clone());
            self.cursor = frame.cursor.stats();
            self.cursor_source = frame.cursor.message_pre_read_source(&frame.body);
            self.last_slot = frame.slots.last_slot();
            self.waiting = frame.waiting.is_some();
            self.par_components = frame.par_components;
            self.par_locally_free_range = frame.par_locally_free_range;
            self.seen.extend_from_slice(frame.slots.seen_slice());
            self.slot_lengths
                .extend(frame.semantic_slots.iter().map(Vec::len));
        }
    }

    fn matches_checked_field_prestate(
        &self,
        field: &MessageFieldSourceReceipt<'src>,
        frame: &MessageFrame<'src>,
    ) -> bool {
        let slot = field.resolved.slot();
        self.program
            .is_some_and(|program| std::ptr::eq(program, field.program))
            && self
                .body
                .as_ref()
                .is_some_and(|body| body.same_view(&field.body))
            && !self.waiting
            && self.cursor == field.cursor_before
            && self.cursor_source.is_some_and(|source| {
                source.reader_at == source.cursor_yielded
                    && source.reader_at == self.cursor.bytes_read
                    && source.fields_read == self.cursor.fields_read
            })
            && field.cursor_before == field.prestate.cursor
            && self.last_slot == field.last_slot_before
            && field.last_slot_before == field.prestate.last_slot
            && self.par_components == field.par_components_before
            && self.par_locally_free_range == field.par_locally_free_before
            && self.seen.len() == field.slot_count
            && self.slot_lengths.len() == field.slot_count
            && self.seen.get(slot).copied() == Some(field.seen_before)
            && field.seen_before == field.prestate.seen
            && self.slot_lengths.get(slot).copied() == Some(field.slot_occurrences_before)
            && frame.slots.len() == self.seen.len()
            && frame.semantic_slots.len() == self.slot_lengths.len()
    }

    fn matches_checked_scalar_receipt(
        &self,
        field: &MessageFieldSourceReceipt<'src>,
        frame: &MessageFrame<'src>,
    ) -> bool {
        let slot = field.resolved.slot();
        self.matches_checked_field_prestate(field, frame)
            && self
                .seen
                .iter()
                .enumerate()
                .all(|(index, seen)| frame.slots.seen_at(index) == Some(index == slot || *seen))
            && self.slot_lengths.iter().enumerate().all(|(index, length)| {
                length.checked_add(usize::from(index == slot))
                    == Some(frame.semantic_slots[index].len())
            })
            && field.matches_frame(frame, &FrameAction::Continue)
    }

    fn matches_checked_push_receipt(
        &self,
        field: &MessageFieldSourceReceipt<'src>,
        frames: &[Frame<'src>],
        pushed: &FramePushObservation,
    ) -> bool {
        let Some(parent_index) = pushed.before_len.checked_sub(1) else {
            return false;
        };
        let Some(Frame::Message(parent)) = frames.get(parent_index) else {
            return false;
        };
        let slot = field.resolved.slot();
        let waiting_matches = match (&parent.waiting, pushed.request_kind) {
            (
                Some(MessageWait::Child {
                    slot: waiting_slot,
                    variant,
                    body,
                    field_position,
                }),
                FrameRequestKind::Message,
            ) => {
                let expected_variant = match field.resolved.payload() {
                    ResolvedProtobufPayload::Declared(_) => None,
                    ResolvedProtobufPayload::Oneof(arm) => Some(arm.name),
                };
                *waiting_slot == slot
                    && *variant == expected_variant
                    && *field_position == field.field_position
                    && matches!(&field.field.value,
                        IndexedCanonicalFieldValue::LengthDelimited(payload)
                            if body.same_view(payload))
            }
            (
                Some(MessageWait::Map {
                    slot: waiting_slot,
                    field_position,
                }),
                FrameRequestKind::Map,
            )
            | (
                Some(MessageWait::EPathMap {
                    slot: waiting_slot,
                    field_position,
                }),
                FrameRequestKind::EPathMap,
            ) => *waiting_slot == slot && *field_position == field.field_position,
            _ => false,
        };
        self.matches_checked_field_prestate(field, parent)
            && waiting_matches
            && self
                .seen
                .iter()
                .enumerate()
                .all(|(index, seen)| parent.slots.seen_at(index) == Some(index == slot || *seen))
            && self
                .slot_lengths
                .iter()
                .enumerate()
                .all(|(index, length)| parent.semantic_slots[index].len() == *length)
            && field.slot_occurrences_after == field.slot_occurrences_before
            && field.child_kind == Some(pushed.request_kind)
            && field.waiting_after == Some(pushed.request_kind)
            && field.matches_pushed_driver(frames, pushed)
    }
}

struct MessageFieldSourceReceipt<'src> {
    program: &'static RhoMatchProgram,
    body: AdmissionRegion<'src>,
    region_start: CanonicalByteProvenance,
    field: IndexedCanonicalField<'src>,
    program_type_id: u16,
    field_number: u32,
    field_position: CanonicalByteProvenance,
    wire: CanonicalWireType,
    encoded_range: CanonicalByteRange,
    resolved: ResolvedProtobufField,
    oneof_arm: Option<u32>,
    slot_count: usize,
    last_slot_before: Option<usize>,
    seen_before: bool,
    slot_occurrences_before: usize,
    par_components_before: u64,
    par_locally_free_before: Option<CanonicalByteRange>,
    payload_len: Option<usize>,
    payload_position: Option<CanonicalByteProvenance>,
    cursor_before: IndexedCanonicalFieldCursorStats,
    prestate: MessageFieldPrestateWitness,
    cursor_after: IndexedCanonicalFieldCursorStats,
    last_slot_after: Option<usize>,
    seen_after: bool,
    slot_occurrences_after: usize,
    waiting_after: Option<FrameRequestKind>,
    child_kind: Option<FrameRequestKind>,
    child_type_id: Option<u16>,
    child_region_start: Option<CanonicalByteProvenance>,
    child_region_len: Option<usize>,
    map_request: Option<FrameRequest<'src>>,
}

struct MessageFieldBefore<'src> {
    body: AdmissionRegion<'src>,
    region_start: CanonicalByteProvenance,
    last_slot: Option<usize>,
    seen: bool,
    slot_occurrences: usize,
    par_components: u64,
    par_locally_free: Option<CanonicalByteRange>,
    field_position: CanonicalByteProvenance,
    cursor: IndexedCanonicalFieldCursorStats,
}

struct MessageEofSourceReceipt<'src> {
    program: &'static RhoMatchProgram,
    body: AdmissionRegion<'src>,
    region_start: CanonicalByteProvenance,
    program_type_id: u16,
    selected_kind: SemanticKind,
    component: Option<ComponentFinishObservation>,
    cursor_after: IndexedCanonicalFieldCursorStats,
    score_len_before: usize,
    score_len_after: usize,
    summary_source: SemanticSummarySource<'src>,
}

struct MessageAcceptBefore<'src> {
    program: &'static RhoMatchProgram,
    body: AdmissionRegion<'src>,
    child_body: AdmissionRegion<'src>,
    cursor: IndexedCanonicalFieldCursorStats,
    last_slot: Option<usize>,
    slot: usize,
    occurrences: usize,
    field_position: CanonicalByteProvenance,
    child_kind: FrameRequestKind,
    returned: FrameSummarySource<'src>,
    stack_len: usize,
    score_len_before_complete: usize,
    score_len_after_finish: usize,
}

struct MessageAcceptSourceReceipt<'src> {
    before: MessageAcceptBefore<'src>,
    cursor_after: IndexedCanonicalFieldCursorStats,
    occurrences_after: usize,
    stack_len_after: usize,
    score_len_after_accept: usize,
}

impl<'src> MessageAcceptBefore<'src> {
    fn capture(
        frame: &MessageFrame<'src>,
        child: &Frame<'src>,
        summary: &FrameSummary<'src>,
        stack_len: usize,
        score_len_before_complete: usize,
        score_len_after_finish: usize,
    ) -> Option<Self> {
        let child_body = match child {
            Frame::Message(child) => &child.body,
            Frame::Map(child) => &child.region,
            Frame::EPathMap(child) => &child.snapshot,
            Frame::CanonicalPath(_) => return None,
        };
        let (slot, field_position, child_kind) = match frame.waiting.as_ref()? {
            MessageWait::Child {
                slot,
                body,
                field_position,
                ..
            } if body.same_view(child_body) => (*slot, *field_position, FrameRequestKind::Message),
            MessageWait::Map {
                slot,
                field_position,
            } => (*slot, *field_position, FrameRequestKind::Map),
            MessageWait::EPathMap {
                slot,
                field_position,
            } => (*slot, *field_position, FrameRequestKind::EPathMap),
            _ => return None,
        };
        if child.request_kind() != child_kind
            || summary.request_kind() != child_kind
            || frame.slots.last_slot() != Some(slot)
            || frame.slots.seen_at(slot) != Some(true)
        {
            return None;
        }
        let occurrences = frame.semantic_slots.get(slot)?.len();
        Some(Self {
            program: frame.program,
            body: frame.body.clone(),
            child_body: child_body.clone(),
            cursor: frame.cursor.stats(),
            last_slot: frame.slots.last_slot(),
            slot,
            occurrences,
            field_position,
            child_kind,
            returned: FrameSummarySource::before(summary),
            stack_len,
            score_len_before_complete,
            score_len_after_finish,
        })
    }

    fn after(
        self,
        frame: &MessageFrame<'src>,
        stack_len_after: usize,
        scores: &CanonicalScoreArena<'src>,
    ) -> Option<MessageAcceptSourceReceipt<'src>> {
        let receipt = MessageAcceptSourceReceipt {
            cursor_after: frame.cursor.stats(),
            occurrences_after: frame.semantic_slots.get(self.slot)?.len(),
            stack_len_after,
            score_len_after_accept: scores.source_len(),
            before: self,
        };
        receipt.matches_frame(frame).then_some(receipt)
    }
}

impl MessageAcceptSourceReceipt<'_> {
    fn matches_frame(&self, frame: &MessageFrame<'_>) -> bool {
        std::ptr::eq(self.before.program, frame.program)
            && self.before.body.same_view(&frame.body)
            && self.before.cursor == self.cursor_after
            && self.cursor_after == frame.cursor.stats()
            && self.before.last_slot == frame.slots.last_slot()
            && self.before.child_kind != FrameRequestKind::CanonicalPath
            && self.before.score_len_before_complete <= self.before.score_len_after_finish
            && self.before.score_len_after_finish == self.score_len_after_accept
            && self.before.stack_len.checked_sub(1) == Some(self.stack_len_after)
            && frame.waiting.is_none()
            && frame.slots.seen_at(self.before.slot) == Some(true)
            && self.before.occurrences.checked_add(1) == Some(self.occurrences_after)
            && frame
                .semantic_slots
                .get(self.before.slot)
                .is_some_and(|occurrences| {
                    occurrences.last().is_some_and(|occurrence| {
                        occurrence.field_position == self.before.field_position
                            && self.before.returned.matches_stored(&occurrence.value)
                            && match &occurrence.body {
                                Some(body) => self.before.child_body.same_view(body),
                                None => self.before.child_kind == FrameRequestKind::Map,
                            }
                    })
                })
    }
}

/// Keeps the exact semantic frame borrowed across its finish reducer. The
/// reducer may append scores, but it cannot change the frame while this source
/// view exists. Only the arena length is retained; its entries are append-only.
struct MessageFinishSource<'frame, 'src> {
    frame: &'frame MessageFrame<'src>,
    selected_kind: SemanticKind,
    cursor: IndexedCanonicalFieldCursorStats,
    score_len_before: usize,
}

impl<'frame, 'src> MessageFinishSource<'frame, 'src> {
    fn before(
        frame: &'frame MessageFrame<'src>,
        scores: &CanonicalScoreArena<'src>,
    ) -> Result<Self, CanonicalProtobufAdmissionError> {
        let selected_kind = semantic_kind(frame.program)
            .ok_or_else(|| frame.schema_error(0, "message has no canonical semantic reducer"))?;
        Ok(Self {
            frame,
            selected_kind,
            cursor: frame.cursor.stats(),
            score_len_before: scores.source_len(),
        })
    }

    fn matches_finish(
        &self,
        finished: &MessageFinishObservation<'src>,
        scores: &CanonicalScoreArena<'src>,
    ) -> bool {
        self.frame.waiting.is_none()
            && self.frame.cursor.is_empty()
            && self.cursor == self.frame.cursor.stats()
            && self.cursor.bytes_read == self.frame.body.len()
            && self.selected_kind == finished.selected_kind
            && finished.summary.semantic.matches_kind(self.selected_kind)
            && self.score_len_before <= scores.source_len()
            && finished.component().is_none_or(|component| {
                if matches!(self.selected_kind, SemanticKind::Component(_)) {
                    self.score_len_before < scores.source_len()
                        && component.score_handle.index().checked_add(1)
                            == Some(scores.source_len())
                } else {
                    component.score_handle.index() < scores.source_len()
                }
            })
    }
}

struct MapFieldObservation<'src> {
    owner: &'static RhoMatchProgram,
    outer_field: u32,
    value_type_id: u16,
    body: AdmissionRegion<'src>,
    region_start: CanonicalByteProvenance,
    field: IndexedCanonicalField<'src>,
    key_before: bool,
    key_position_before: Option<CanonicalByteProvenance>,
    saw_value_before: bool,
    value_position_before: Option<CanonicalByteProvenance>,
    value_absent_before: bool,
    waiting_before: bool,
    field_number: u32,
    wire: CanonicalWireType,
    encoded_range: CanonicalByteRange,
    field_position: CanonicalByteProvenance,
    payload_len: usize,
    payload_position: CanonicalByteProvenance,
    cursor_last_field_before: u32,
    cursor_before: IndexedCanonicalFieldCursorStats,
    cursor_after: IndexedCanonicalFieldCursorStats,
}

struct MapEofObservation<'src> {
    owner: &'static RhoMatchProgram,
    outer_field: u32,
    value_type_id: u16,
    body: AdmissionRegion<'src>,
    region_start: CanonicalByteProvenance,
    cursor_after: IndexedCanonicalFieldCursorStats,
    key_present: bool,
    key_position: Option<CanonicalByteProvenance>,
    saw_value: bool,
    value_position: Option<CanonicalByteProvenance>,
    summary: MapSummarySource<'src>,
}

struct MapAcceptObservation {
    score: CanonicalScoreHandle,
    canonical_path_stable: bool,
    identity_prefix_len: usize,
    identity_suffix_len: usize,
    value_position: Option<CanonicalByteProvenance>,
}

impl MapAcceptObservation {
    fn matches_frame(&self, frame: &MapFrame<'_>) -> bool {
        frame.saw_value
            && !frame.waiting_value
            && frame.value_position == self.value_position
            && frame.value.as_ref().is_some_and(|value| {
                value.score == self.score
                    && !value.semantic_default
                    && value.canonical_path_stable == self.canonical_path_stable
                    && value.identity_prefix.len() == self.identity_prefix_len
                    && value.identity_suffix.len() == self.identity_suffix_len
            })
    }
}

impl MapFieldObservation<'_> {
    fn matches_frame(&self, frame: &MapFrame<'_>, action: &FrameAction<'_>) -> bool {
        let action_matches = match (self.field_number, action) {
            (1, FrameAction::Continue) => {
                !self.key_before
                    && self.key_position_before.is_none()
                    && !self.saw_value_before
                    && self.value_position_before.is_none()
                    && self.value_absent_before
                    && !self.waiting_before
                    && self.cursor_last_field_before == 0
                    && !frame.waiting_value
                    && frame.saw_value == self.saw_value_before
                    && !frame.saw_value
                    && frame.value_position == self.value_position_before
                    && frame.value.is_none()
                    && frame.key_position == Some(self.field_position)
                    && frame.key.as_ref().is_some_and(|key| {
                        matches!(
                            &self.field.value,
                            IndexedCanonicalFieldValue::LengthDelimited(region)
                                if key.same_view(region)
                        ) && key.len() == self.payload_len
                            && key.position_at(0).map(|position| position.physical)
                                == Some(self.payload_position)
                    })
            }
            (2, FrameAction::Push(FrameRequest::Message { region, program })) => {
                !self.saw_value_before
                    && self.value_absent_before
                    && frame.value.is_none()
                    && self.cursor_last_field_before == (if self.key_before { 1 } else { 0 })
                    && !self.waiting_before
                    && frame.saw_value
                    && frame.waiting_value
                    && frame.key.is_some() == self.key_before
                    && frame.key_position == self.key_position_before
                    && frame.value_position == Some(self.field_position)
                    && program.type_id == frame.value_type_id
                    && matches!(
                        &self.field.value,
                        IndexedCanonicalFieldValue::LengthDelimited(value)
                            if region.same_view(value)
                    )
                    && region.len() == self.payload_len
                    && region.position_at(0).map(|position| position.physical)
                        == Some(self.payload_position)
            }
            _ => false,
        };
        std::ptr::eq(self.owner, frame.owner)
            && self.outer_field == frame.outer_field
            && self.value_type_id == frame.value_type_id
            && self.body.same_view(&frame.region)
            && self.region_start == frame.region_start
            && frame.cursor.matches_successful_ordered_field(
                &frame.region,
                &self.field,
                self.cursor_last_field_before,
                self.cursor_before.fields_read,
            )
            && self.field.number == self.field_number
            && self.field.wire_type == self.wire
            && self.field.encoded_range == self.encoded_range
            && self.field.field_start.logical_offset == self.encoded_range.start
            && self.field.field_start.physical == self.field_position
            && self
                .body
                .position_at(self.encoded_range.start)
                .map(|position| position.physical)
                == Some(self.field_position)
            && self.wire == CanonicalWireType::LengthDelimited
            && self.encoded_range.start < self.encoded_range.end
            && self.encoded_range.end <= self.body.len()
            && self.cursor_before.bytes_read == self.encoded_range.start
            && self.cursor_after.bytes_read == self.encoded_range.end
            && self.cursor_before.fields_read.checked_add(1) == Some(self.cursor_after.fields_read)
            && self.cursor_after == frame.cursor.stats()
            && action_matches
    }

    fn matches_pushed_driver(&self, frames: &[Frame<'_>], pushed: &FramePushObservation) -> bool {
        let Some(parent_index) = pushed.before_len.checked_sub(1) else {
            return false;
        };
        let (Some(Frame::Map(parent)), Some(child @ Frame::Message(_))) =
            (frames.get(parent_index), frames.get(pushed.before_len))
        else {
            return false;
        };
        let Frame::Message(child_message) = child else {
            unreachable!()
        };
        let request = FrameRequest::Message {
            region: child_message.body.clone(),
            program: child_message.program,
        };
        pushed.request_kind == FrameRequestKind::Message
            && pushed.after_len == frames.len()
            && request.matches_pushed_frame(child)
            && self.matches_frame(parent, &FrameAction::Push(request))
    }
}

impl<'src> MapEofObservation<'src> {
    fn from_frame(frame: &MapFrame<'src>) -> Self {
        let key = frame.key.clone().unwrap_or_else(|| {
            frame
                .region
                .subview(CanonicalByteRange::default())
                .expect("an empty range is inside every map entry")
        });
        Self {
            owner: frame.owner,
            outer_field: frame.outer_field,
            value_type_id: frame.value_type_id,
            body: frame.region.clone(),
            region_start: frame.region_start,
            cursor_after: frame.cursor.stats(),
            key_present: frame.key.is_some(),
            key_position: frame.key_position,
            saw_value: frame.saw_value,
            value_position: frame.value_position,
            summary: MapSummarySource {
                key,
                key_position: frame.key_position.unwrap_or(frame.region_start),
                value: frame.value.as_ref().map(ParSummarySource::before),
            },
        }
    }

    fn matches_frame(&self, frame: &MapFrame<'src>, action: &FrameAction<'src>) -> bool {
        let FrameAction::Complete(FrameSummary::Map(summary)) = action else {
            return false;
        };
        std::ptr::eq(self.owner, frame.owner)
            && self.outer_field == frame.outer_field
            && self.value_type_id == frame.value_type_id
            && self.body.same_view(&frame.region)
            && self.region_start == frame.region_start
            && self.key_present == frame.key.is_some()
            && self.key_position == frame.key_position
            && self.saw_value == frame.saw_value
            && self.value_position == frame.value_position
            && self.cursor_after == frame.cursor.stats()
            && frame.cursor.is_empty()
            && frame.cursor.is_terminal_error_free()
            && self.cursor_after.bytes_read == frame.region.len()
            && !frame.waiting_value
            && self.summary.matches(summary)
    }

    fn matches_source(&self, other: &Self) -> bool {
        std::ptr::eq(self.owner, other.owner)
            && self.outer_field == other.outer_field
            && self.value_type_id == other.value_type_id
            && self.body.same_view(&other.body)
            && self.region_start == other.region_start
            && self.cursor_after == other.cursor_after
            && self.key_present == other.key_present
            && self.key_position == other.key_position
            && self.saw_value == other.saw_value
            && self.value_position == other.value_position
            && self.summary.matches_source(&other.summary)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MapPushViewIdentityClasses {
    selected: usize,
    requested: usize,
    retained: usize,
}

impl MapPushViewIdentityClasses {
    fn capture<'src>(
        selected: &AdmissionRegion<'src>,
        requested: &AdmissionRegion<'src>,
        retained: &AdmissionRegion<'src>,
    ) -> Self {
        let requested_class = if selected.same_view(requested) { 0 } else { 1 };
        let retained_class = if selected.same_view(retained) {
            0
        } else if requested_class == 1 && requested.same_view(retained) {
            1
        } else {
            2
        };
        Self {
            selected: 0,
            requested: requested_class,
            retained: retained_class,
        }
    }

    fn all_same(self) -> bool { self.selected == self.requested && self.requested == self.retained }
}

struct MapPushCheckedViewObservation<'view, 'src> {
    selected: &'view AdmissionRegion<'src>,
    requested: &'view AdmissionRegion<'src>,
    retained: &'view AdmissionRegion<'src>,
    classes: MapPushViewIdentityClasses,
}

impl<'view, 'src> MapPushCheckedViewObservation<'view, 'src> {
    fn capture(
        selected: &'view AdmissionRegion<'src>,
        requested: &'view AdmissionRegion<'src>,
        retained: &'view AdmissionRegion<'src>,
    ) -> Self {
        Self {
            selected,
            requested,
            retained,
            classes: MapPushViewIdentityClasses::capture(selected, requested, retained),
        }
    }

    fn all_same(&self) -> bool {
        debug_assert_eq!(
            self.classes.selected == self.classes.requested,
            self.selected.same_view(self.requested),
        );
        debug_assert_eq!(
            self.classes.requested == self.classes.retained,
            self.requested.same_view(self.retained),
        );
        debug_assert_eq!(
            self.classes.selected == self.classes.retained,
            self.selected.same_view(self.retained),
        );
        self.classes.all_same()
    }

    #[cfg(test)]
    fn indexed_images(&self) -> [MapPushIndexedImage; 3] {
        [self.selected, self.requested, self.retained].map(MapPushIndexedImage::capture)
    }
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct MapPushIndexedImage {
    located: Vec<CanonicalLocatedByte>,
    end: CanonicalByteProvenance,
}

#[cfg(test)]
impl MapPushIndexedImage {
    fn capture(region: &AdmissionRegion<'_>) -> Self {
        Self {
            located: region.located_bytes().collect(),
            end: region.end_position().physical,
        }
    }
}

struct MapPushCheckedChildObservation<'view, 'src> {
    #[cfg(test)]
    owner_type_id: u16,
    #[cfg(test)]
    outer_field: u32,
    #[cfg(test)]
    value_type_id: u16,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "only the test-only indexed_image audit and the property tests read this view, but production builds keep it because it is the struct's only use of the 'view and 'src lifetimes"
        )
    )]
    region: &'view AdmissionRegion<'src>,
    #[cfg(test)]
    physical_start: CanonicalByteProvenance,
    cursor_at: usize,
    key_present: bool,
    key_position_present: bool,
    saw_value: bool,
    value_position_present: bool,
    value_present: bool,
    waiting_value: bool,
}

struct MapPushCheckedParentCoreObservation<'view, 'src> {
    body: &'view AdmissionRegion<'src>,
    seen: &'view [bool],
    semantic_slots: &'view [Vec<SemanticOccurrence<'src>>],
    slot: usize,
    seen_before: bool,
}

impl<'view, 'src> MapPushCheckedParentCoreObservation<'view, 'src> {
    fn capture(
        parent: &'view MessageFrame<'src>,
        field: &MessageFieldSourceReceipt<'src>,
    ) -> Option<Self> {
        let slot = field.resolved.slot();
        let Some(MessageWait::Map { slot: waiting, .. }) = parent.waiting.as_ref() else {
            return None;
        };
        if *waiting != slot
            || field.slot_count != parent.program.fields.len()
            || parent.slots.len() != parent.semantic_slots.len()
            || parent.slots.len() != field.slot_count
            || parent.slots.seen_at(slot) != Some(true)
            || parent.semantic_slots.get(slot)?.len() != field.slot_occurrences_before
        {
            return None;
        }
        Some(Self {
            body: &parent.body,
            seen: parent.slots.seen_slice(),
            semantic_slots: &parent.semantic_slots,
            slot,
            seen_before: field.seen_before,
        })
    }

    fn seen_before_at(&self, slot: usize) -> Option<bool> {
        self.seen.get(slot).map(|seen| {
            if slot == self.slot {
                self.seen_before
            } else {
                *seen
            }
        })
    }
}

impl<'view, 'src> MapPushCheckedChildObservation<'view, 'src> {
    fn capture(child: &'view MapFrame<'src>) -> Self {
        Self {
            #[cfg(test)]
            owner_type_id: child.owner.type_id,
            #[cfg(test)]
            outer_field: child.outer_field,
            #[cfg(test)]
            value_type_id: child.value_type_id,
            region: &child.region,
            #[cfg(test)]
            physical_start: child.region_start,
            cursor_at: child.cursor.logical_offset(),
            key_present: child.key.is_some(),
            key_position_present: child.key_position.is_some(),
            saw_value: child.saw_value,
            value_position_present: child.value_position.is_some(),
            value_present: child.value.is_some(),
            waiting_value: child.waiting_value,
        }
    }

    fn from_pushed(request: &FrameRequest<'src>, child: &'view Frame<'src>) -> Option<Self> {
        if !request.matches_pushed_frame(child) {
            return None;
        }
        let Frame::Map(child) = child else {
            return None;
        };
        Some(Self::capture(child))
    }

    fn is_initial(&self) -> bool {
        self.cursor_at == 0
            && !self.key_present
            && !self.key_position_present
            && !self.saw_value
            && !self.value_position_present
            && !self.value_present
            && !self.waiting_value
    }

    #[cfg(test)]
    fn indexed_image(&self) -> MapPushIndexedImage { MapPushIndexedImage::capture(self.region) }
}

impl<'src> MessageFieldSourceReceipt<'src> {
    /// Re-read only the two bounded varints of a scalar Continue from the
    /// retained pre-read body. The receipt's field value is not the source of
    /// truth for this check; the indexed bytes are.
    fn matches_varint_source_read(&self) -> bool {
        let IndexedCanonicalFieldValue::Varint(recorded) = &self.field.value else {
            return true;
        };
        let Some(field_view) = self.body.subview(self.encoded_range) else {
            return false;
        };
        let mut reader = field_view.reader();
        let Ok(key) = reader.read_canonical_varint() else {
            return false;
        };
        if key != (u64::from(self.field.number) << 3)
            || self.field.wire_type != CanonicalWireType::Varint
        {
            return false;
        }
        reader
            .read_canonical_varint()
            .is_ok_and(|value| value == *recorded)
            && reader.is_empty()
    }

    fn map_parent_logical_bounds(&self, parent: &MessageFrame<'_>) -> Option<CanonicalByteRange> {
        (self.body.same_view(&parent.body)
            && parent.cursor.matches_message_view(&parent.body)
            && self.cursor_after == parent.cursor.stats()
            && self.encoded_range.end <= parent.body.len())
        .then_some(CanonicalByteRange {
            start: 0,
            end: parent.body.len(),
        })
    }

    fn map_selected_subview(&self) -> Option<(CanonicalByteRange, AdmissionRegion<'src>)> {
        let IndexedCanonicalFieldValue::LengthDelimited(selected) = &self.field.value else {
            return None;
        };
        let end = self.encoded_range.end;
        let start = end.checked_sub(selected.len())?;
        if start <= self.encoded_range.start {
            return None;
        }
        let range = CanonicalByteRange { start, end };
        let source = self.body.subview(range)?;
        source.same_view(selected).then_some((range, source))
    }

    fn matches_pushed_driver(&self, frames: &[Frame<'_>], pushed: &FramePushObservation) -> bool {
        let Some(parent_index) = pushed.before_len.checked_sub(1) else {
            return false;
        };
        let (Some(Frame::Message(parent)), Some(child)) = (frames.get(parent_index), frames.last())
        else {
            return false;
        };
        let child_region = match child {
            Frame::Message(frame) => &frame.body,
            Frame::Map(frame) => &frame.region,
            Frame::EPathMap(frame) => &frame.snapshot,
            Frame::CanonicalPath(frame) => &frame.input,
        };
        let IndexedCanonicalFieldValue::LengthDelimited(payload) = &self.field.value else {
            return false;
        };
        self.child_kind == Some(pushed.request_kind)
            && self.child_kind == Some(child.request_kind())
            && match self.child_kind {
                Some(FrameRequestKind::Map) => self.map_request.as_ref().is_some_and(|request| {
                    request.request_kind() == FrameRequestKind::Map
                        && self.map_parent_logical_bounds(parent).is_some()
                        && self.map_selected_subview().is_some()
                        && MapPushCheckedViewObservation::capture(
                            payload,
                            request.region(),
                            child_region,
                        )
                        .all_same()
                        && MapPushCheckedChildObservation::from_pushed(request, child)
                            .is_some_and(|observed| observed.is_initial())
                }),
                _ => self.map_request.is_none(),
            }
            && self.program_type_id == parent.program.type_id
            && self.last_slot_after == parent.slots.last_slot()
            && parent.slots.seen_at(self.resolved.slot()) == Some(self.seen_after)
            && self.cursor_after == parent.cursor.stats()
            && (self.child_kind == Some(FrameRequestKind::Map) || payload.same_view(child_region))
            && self.child_region_start
                == child_region
                    .position_at(0)
                    .map(|position| position.physical)
            && self.child_region_len == Some(child_region.len())
            && payload.end_position().physical == child_region.end_position().physical
            && pushed.before_len.checked_add(1) == Some(pushed.after_len)
            && pushed.after_len == frames.len()
            && pushed.before_capacity <= pushed.after_capacity
            && pushed.used_bytes <= pushed.reserved_bytes
            && pushed.frame_variant_bytes == pushed.stats_frame_variant_bytes
    }

    fn matches_frame(&self, frame: &MessageFrame<'_>, action: &FrameAction<'_>) -> bool {
        let slot = self.resolved.slot();
        let raw_par_locally_free = frame.is_raw_par()
            && frame
                .program
                .fields
                .get(slot)
                .is_some_and(|field| field.name == "locally_free");
        let expected_variant = match self.resolved.payload() {
            ResolvedProtobufPayload::Declared(_) => None,
            ResolvedProtobufPayload::Oneof(arm) => Some(arm.name),
        };
        let expected_oneof_arm = match self.resolved.payload() {
            ResolvedProtobufPayload::Declared(_) => None,
            ResolvedProtobufPayload::Oneof(arm) => Some(arm.tag),
        };
        let expected_child_kind = match self.resolved.payload() {
            ResolvedProtobufPayload::Oneof(arm) => match arm.payload {
                RhoMatchPayloadKind::Message(_) => Some(FrameRequestKind::Message),
                _ => None,
            },
            ResolvedProtobufPayload::Declared(_) => match self.resolved.field().kind {
                RhoMatchFieldKind::OptionalMessage(_) | RhoMatchFieldKind::MessageSequence(_) => {
                    Some(FrameRequestKind::Message)
                }
                RhoMatchFieldKind::StringMessageMap(_) => Some(FrameRequestKind::Map),
                RhoMatchFieldKind::PathMap => Some(FrameRequestKind::EPathMap),
                _ => None,
            },
        };
        let expected_child_type_id = match self.resolved.payload() {
            ResolvedProtobufPayload::Oneof(arm) => match arm.payload {
                RhoMatchPayloadKind::Message(type_id) => Some(type_id),
                _ => None,
            },
            ResolvedProtobufPayload::Declared(_) => match self.resolved.field().kind {
                RhoMatchFieldKind::OptionalMessage(type_id)
                | RhoMatchFieldKind::MessageSequence(type_id)
                | RhoMatchFieldKind::StringMessageMap(type_id) => Some(type_id),
                _ => None,
            },
        };
        let tag_matches = match self.resolved.payload() {
            ResolvedProtobufPayload::Declared(_) => self.resolved.field().tag == self.field_number,
            ResolvedProtobufPayload::Oneof(arm) => arm.tag == self.field_number,
        };
        let wire_matches = self
            .resolved
            .protobuf_kind()
            .wire_type()
            .map(canonical_wire_type)
            == Some(self.wire);
        let action_matches = match action {
            FrameAction::Continue => {
                self.child_kind.is_none()
                    && self.matches_varint_source_read()
                    && self.child_type_id.is_none()
                    && self.child_region_start.is_none()
                    && self.child_region_len.is_none()
                    && self.waiting_after.is_none()
                    && frame.cursor.matches_message_view(&frame.body)
                    && frame.cursor.logical_offset() == self.encoded_range.end
                    && frame.cursor.logical_offset() == self.cursor_after.bytes_read
                    && frame.waiting.is_none()
                    && frame.par_components == self.par_components_before
                    && frame.par_locally_free_range
                        == if raw_par_locally_free {
                            Some(self.encoded_range)
                        } else {
                            self.par_locally_free_before
                        }
                    && frame.semantic_slots.get(slot).is_some_and(|occurrences| {
                        self.slot_occurrences_before.checked_add(1) == Some(occurrences.len())
                            && occurrences.last().is_some_and(|occurrence| {
                                occurrence.variant == expected_variant
                                    && occurrence.body.is_none()
                                    && match &occurrence.value {
                                        SemanticValue::Scalar(actual) => semantic_scalar(
                                            &self.field,
                                            self.resolved.protobuf_kind(),
                                        )
                                        .is_ok_and(|expected| expected.same_source_value(actual)),
                                        _ => false,
                                    }
                                    && occurrence.field_position == self.field_position
                            })
                    })
            }
            FrameAction::Push(request) => {
                let request_type_id = match request {
                    FrameRequest::Message { program, .. } => Some(program.type_id),
                    FrameRequest::Map { value_type_id, .. } => Some(*value_type_id),
                    FrameRequest::EPathMap { .. } | FrameRequest::CanonicalPath(_) => None,
                };
                let pending_matches = match &frame.waiting {
                    Some(MessageWait::Child {
                        slot: waiting,
                        variant,
                        body,
                        field_position,
                    }) => {
                        *waiting == slot
                            && *variant == expected_variant
                            && body.same_view(request.region())
                            && *field_position == self.field_position
                            && request.request_kind() == FrameRequestKind::Message
                    }
                    Some(MessageWait::Map {
                        slot: waiting,
                        field_position,
                    }) => {
                        *waiting == slot
                            && *field_position == self.field_position
                            && matches!(request, FrameRequest::Map { owner, field, .. }
                                if std::ptr::eq(*owner, frame.program)
                                    && *field == self.field_number)
                    }
                    Some(MessageWait::EPathMap {
                        slot: waiting,
                        field_position,
                    }) => {
                        *waiting == slot
                            && *field_position == self.field_position
                            && matches!(request, FrameRequest::EPathMap { owner, field, .. }
                                if std::ptr::eq(*owner, frame.program)
                                    && *field == self.field_number)
                    }
                    None => false,
                };
                let expected_par_components = if frame.is_raw_par()
                    && matches!(
                        self.resolved.field().kind,
                        RhoMatchFieldKind::MessageSequence(_)
                    ) {
                    self.par_components_before.checked_add(1)
                } else {
                    Some(self.par_components_before)
                };
                self.child_kind == Some(request.request_kind())
                    && self.waiting_after == self.child_kind
                    && self.child_type_id == request_type_id
                    && self.child_type_id == expected_child_type_id
                    && self.child_region_len == Some(request.region().len())
                    && self.child_region_start
                        == request
                            .region()
                            .position_at(0)
                            .map(|position| position.physical)
                    && pending_matches
                    && expected_par_components == Some(frame.par_components)
                    && frame.par_locally_free_range == self.par_locally_free_before
                    && frame.semantic_slots.get(slot).is_some_and(|occurrences| {
                        occurrences.len() == self.slot_occurrences_before
                    })
                    && matches!(
                        &self.field.value,
                        IndexedCanonicalFieldValue::LengthDelimited(region)
                            if region.same_view(request.region())
                    )
                    && self.payload_len == Some(request.region().len())
                    && self.payload_position
                        == request
                            .region()
                            .position_at(0)
                            .map(|position| position.physical)
            }
            FrameAction::Complete(_) => false,
        };
        std::ptr::eq(frame.program, self.program)
            && frame.program.type_id == self.program_type_id
            && frame.body.same_view(&self.body)
            && frame.region_start == self.region_start
            && self.field.number == self.field_number
            && self.field.wire_type == self.wire
            && self.field.encoded_range == self.encoded_range
            && self.field.field_start.logical_offset == self.encoded_range.start
            && self.field.field_start.physical == self.field_position
            && self
                .body
                .position_at(self.encoded_range.start)
                .map(|position| position.physical)
                == Some(self.field_position)
            && frame
                .program
                .fields
                .get(slot)
                .is_some_and(|field| std::ptr::eq(field, self.resolved.field()))
            && slot < frame.slots.len()
            && frame.slots.last_slot() == Some(slot)
            && self.last_slot_after == frame.slots.last_slot()
            && frame.slots.seen_at(slot) == Some(true)
            && self.seen_after
                == frame
                    .slots
                    .seen_at(slot)
                    .expect("checked generated slot is in bounds")
            && self.slot_count == frame.slots.len()
            && self.slot_count == frame.program.fields.len()
            && self.slot_count == frame.semantic_slots.len()
            && self.slot_occurrences_after == frame.semantic_slots[slot].len()
            && self.oneof_arm == expected_oneof_arm
            && self.child_kind == expected_child_kind
            && self
                .last_slot_before
                .is_none_or(|previous| previous <= slot)
            && (self.resolved.repeated() || !self.seen_before)
            && tag_matches
            && wire_matches
            && self.encoded_range.start < self.encoded_range.end
            && self.encoded_range.end <= self.body.len()
            && self.cursor_before.bytes_read == self.encoded_range.start
            && self.cursor_after.bytes_read == self.encoded_range.end
            && self.cursor_before.fields_read.checked_add(1) == Some(self.cursor_after.fields_read)
            && self.cursor_after == frame.cursor.stats()
            && action_matches
    }
}

impl<'src> MessageEofSourceReceipt<'src> {
    fn matches_returned_summary(
        &self,
        outcome: &CompleteAdmissionOutcome<'src>,
        source: &DriverCompleteSource<'src>,
        frames: &[Frame<'src>],
        effect: Option<&FrameAcceptEffect<'src>>,
    ) -> bool {
        match (outcome, source.parent.as_ref(), frames.last()) {
            (CompleteAdmissionOutcome::Root(FrameSummary::Message(summary)), None, None) => {
                self.summary_source.matches(&summary.semantic)
            }
            (
                CompleteAdmissionOutcome::AcceptedMessage(_)
                | CompleteAdmissionOutcome::AcceptedOther,
                Some(CompleteParentSource::Message { slot, .. }),
                Some(Frame::Message(parent)),
            ) => parent
                .semantic_slots
                .get(*slot)
                .and_then(|values| values.last())
                .is_some_and(|occurrence| match &occurrence.value {
                    SemanticValue::Message(summary) => {
                        self.summary_source.matches(&summary.semantic)
                    }
                    _ => false,
                }),
            (
                CompleteAdmissionOutcome::AcceptedOther,
                Some(CompleteParentSource::Map { .. }),
                Some(Frame::Map(parent)),
            ) => match (&self.summary_source, parent.value.as_ref()) {
                (SemanticSummarySource::Par(source), Some(value)) => source.matches(value),
                _ => false,
            },
            (
                CompleteAdmissionOutcome::AcceptedOther,
                Some(CompleteParentSource::EPathMap {
                    returned: EPathMapReturnSource::MapValue { .. },
                    ..
                }),
                Some(Frame::EPathMap(_)),
            ) => match (&self.summary_source, effect) {
                (
                    SemanticSummarySource::Par(source),
                    Some(FrameAcceptEffect::EPathMapValue(value)),
                ) => source.matches_source(value),
                _ => false,
            },
            (
                CompleteAdmissionOutcome::AcceptedOther,
                Some(CompleteParentSource::EPathMap {
                    returned:
                        EPathMapReturnSource::Obligation {
                            returned_par: Some(returned),
                            ..
                        },
                    ..
                }),
                Some(Frame::EPathMap(_)),
            ) => match (&self.summary_source, effect) {
                (
                    SemanticSummarySource::Par(source),
                    Some(FrameAcceptEffect::ResolvedObligation {
                        resolution:
                            CanonicalPathObligationResolution::RawPar {
                                canonical_path_stable,
                            },
                        ..
                    }),
                ) => {
                    source.matches_source(returned)
                        && source.canonical_path_stable == *canonical_path_stable
                }
                _ => false,
            },
            (
                CompleteAdmissionOutcome::AcceptedOther,
                Some(CompleteParentSource::CanonicalPath {
                    returned_par: Some(returned),
                    ..
                }),
                Some(Frame::CanonicalPath(_)),
            ) => match (&self.summary_source, effect) {
                (
                    SemanticSummarySource::Par(source),
                    Some(FrameAcceptEffect::ResolvedObligation {
                        resolution:
                            CanonicalPathObligationResolution::RawPar {
                                canonical_path_stable,
                            },
                        ..
                    }),
                ) => {
                    source.matches_source(returned)
                        && source.canonical_path_stable == *canonical_path_stable
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn matches_frame(
        &self,
        frame: &MessageFrame<'src>,
        summary: &FrameSummary<'src>,
        scores: &CanonicalScoreArena<'src>,
    ) -> bool {
        if let FrameSummary::Message(summary) = summary {
            let returned_component = match &summary.semantic {
                SemanticSummary::Component(component) => Some(ComponentFinishObservation {
                    family: component.family,
                    complete: component.complete,
                    canonical_path_stable: component.canonical_path_stable,
                    score_handle: component.score,
                }),
                _ => None,
            };
            std::ptr::eq(frame.program, self.program)
                && frame.program.type_id == self.program_type_id
                && frame.body.same_view(&self.body)
                && frame.region_start == self.region_start
                && summary.semantic.matches_kind(self.selected_kind)
                && self.summary_source.matches(&summary.semantic)
                && frame.waiting.is_none()
                && frame.cursor.is_empty()
                && self.cursor_after == frame.cursor.stats()
                && self.cursor_after.bytes_read == frame.body.len()
                && self.component == returned_component
                && self.score_len_before <= self.score_len_after
                && self.score_len_after == scores.source_len()
        } else {
            false
        }
    }
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
    frame.append_semantic_occurrence(slot, SemanticOccurrence {
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

fn message_step_into<'src>(
    frame: &mut MessageFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    scores: &mut CanonicalScoreArena<'src>,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<AdmissionStepKind, CanonicalProtobufAdmissionError> {
    if frame.waiting.is_some() {
        return Err(frame.schema_error(0, "protobuf message resumed before its child returned"));
    }
    let cursor_before = frame.cursor.stats();
    let field = frame
        .cursor
        .next_field()
        .map_err(|source| framing_error(frame.program.type_name, frame.region_start, source))?;
    let Some(field) = field else {
        return message_finish_step_into(frame, stats, scores, scratch);
    };
    scratch.field_input = Some(field);
    message_field_step_into(frame, stats, cursor_before, scratch)
}

// The field parser's `IndexedCanonicalField` scratch is not live through EOF.
// Outlining this arm keeps it off the stack while semantic reducers run.
#[cfg_attr(debug_assertions, inline(never))]
fn message_finish_step_into<'src>(
    frame: &mut MessageFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    scores: &mut CanonicalScoreArena<'src>,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<AdmissionStepKind, CanonicalProtobufAdmissionError> {
    #[cfg(test)]
    rpb3_root_finish_trace::record_eof(frame, scores);
    let source = MessageFinishSource::before(frame, scores)?;
    let cursor_stats = frame.cursor.stats();
    stats.protobuf_bytes_read = stats
        .protobuf_bytes_read
        .checked_add(cursor_stats.bytes_read)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    stats.protobuf_bytes_traversed = stats
        .protobuf_bytes_traversed
        .checked_add(cursor_stats.bytes_traversed)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let finished = frame.finish(scores)?;
    let checked_component = if std::ptr::eq(frame.program, &SEND_RHO_MATCH_PROGRAM) {
        Some((
            ParComponentTag::Send,
            check_send_append(frame, scores, source.score_len_before)?,
        ))
    } else if std::ptr::eq(frame.program, &RECEIVE_RHO_MATCH_PROGRAM) {
        Some((
            ParComponentTag::Receive,
            check_receive_append(frame, scores, source.score_len_before)?,
        ))
    } else if std::ptr::eq(frame.program, &NEW_RHO_MATCH_PROGRAM) {
        Some((
            ParComponentTag::New,
            check_new_append(frame, scores, source.score_len_before)?,
        ))
    } else if std::ptr::eq(frame.program, &BUNDLE_RHO_MATCH_PROGRAM) {
        Some((
            ParComponentTag::Bundle,
            check_bundle_append(frame, scores, source.score_len_before)?,
        ))
    } else if std::ptr::eq(frame.program, &IF_RHO_MATCH_PROGRAM) {
        Some((
            ParComponentTag::If,
            check_if_append(frame, scores, source.score_len_before)?,
        ))
    } else if std::ptr::eq(frame.program, &MATCH_RHO_MATCH_PROGRAM) {
        Some((
            ParComponentTag::Match,
            check_match_append(frame, scores, source.score_len_before)?,
        ))
    } else if std::ptr::eq(frame.program, &COSTTOKENSTACK_RHO_MATCH_PROGRAM) {
        Some((
            ParComponentTag::CostTokenStack,
            (
                true,
                check_cost_token_stack_append(frame, scores, source.score_len_before)?,
            ),
        ))
    } else {
        None
    };
    if let Some((family, (complete, expected_score))) = checked_component {
        if !finished.component().is_some_and(|component| {
            component.family == family
                && component.complete == complete
                && component.score_handle.index() == expected_score
                && !component.canonical_path_stable
        }) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "component EOF returned a score or completeness bit not selected by its source",
            ));
        }
    }
    let checked_cost_score = if std::ptr::eq(frame.program, &COSTBOUNDSIGNATURE_RHO_MATCH_PROGRAM) {
        Some((
            SemanticKind::CostBoundSignature,
            check_cost_bound_append(frame, scores, source.score_len_before)?,
        ))
    } else if std::ptr::eq(frame.program, &COSTSIGNATURE_RHO_MATCH_PROGRAM) {
        Some((
            SemanticKind::CostSignature,
            check_cost_signature_append(frame, scores, source.score_len_before)?,
        ))
    } else {
        None
    };
    if let Some((kind, expected_score)) = checked_cost_score {
        if finished.selected_kind != kind
            || !matches!(
                &finished.summary.semantic,
                SemanticSummary::CostScore(score) if score.index() == expected_score
            )
        {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "cost EOF returned a score or summary kind not selected by its source",
            ));
        }
    }
    if !source.matches_finish(&finished, scores) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "message finish disagrees with its borrowed EOF source",
        ));
    }
    let source = MessageEofSourceReceipt {
        program: frame.program,
        body: frame.body.clone(),
        region_start: frame.region_start,
        program_type_id: frame.program.type_id,
        selected_kind: finished.selected_kind,
        component: finished.component(),
        cursor_after: frame.cursor.stats(),
        score_len_before: source.score_len_before,
        score_len_after: scores.source_len(),
        summary_source: SemanticSummarySource::before(&finished.summary.semantic),
    };
    let action = finished.into_action()?;
    #[cfg(test)]
    rpb3_root_finish_trace::record_complete(frame, &action, scores);
    let FrameAction::Complete(summary) = &action else {
        unreachable!("message EOF produces only a complete action")
    };
    if !source.matches_frame(frame, summary, scores) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "admission frame action disagrees with its source observation",
        ));
    }
    let FrameAction::Complete(summary) = action else {
        unreachable!("message EOF produces only a complete action")
    };
    scratch.summary = Some(summary);
    scratch.eof = Some(source);
    Ok(AdmissionStepKind::MessageComplete)
}

// Keep the full field observation out of the decoding arm's debug stack frame.
#[cfg_attr(debug_assertions, inline(never))]
fn verify_message_field_action<'src>(
    frame: &MessageFrame<'src>,
    field: &IndexedCanonicalField<'src>,
    resolved: ResolvedProtobufField,
    before: MessageFieldBefore<'src>,
    action: &FrameAction<'src>,
    receipt: &mut Option<MessageFieldSourceReceipt<'src>>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let payload_region = match &field.value {
        IndexedCanonicalFieldValue::Varint(_) => None,
        IndexedCanonicalFieldValue::Fixed64(region)
        | IndexedCanonicalFieldValue::LengthDelimited(region)
        | IndexedCanonicalFieldValue::Fixed32(region) => Some(region),
    };
    let (child_kind, child_type_id, child_region_start, child_region_len) = match action {
        FrameAction::Continue | FrameAction::Complete(_) => (None, None, None, None),
        FrameAction::Push(request) => {
            let type_id = match request {
                FrameRequest::Message { program, .. } => Some(program.type_id),
                FrameRequest::Map { value_type_id, .. } => Some(*value_type_id),
                FrameRequest::EPathMap { .. } | FrameRequest::CanonicalPath(_) => None,
            };
            (
                Some(request.request_kind()),
                type_id,
                request
                    .region()
                    .position_at(0)
                    .map(|position| position.physical),
                Some(request.region().len()),
            )
        }
    };
    let waiting_after = match frame.waiting.as_ref() {
        None => None,
        Some(MessageWait::Child { .. }) => Some(FrameRequestKind::Message),
        Some(MessageWait::Map { .. }) => Some(FrameRequestKind::Map),
        Some(MessageWait::EPathMap { .. }) => Some(FrameRequestKind::EPathMap),
    };
    let source = MessageFieldSourceReceipt {
        program: frame.program,
        body: before.body,
        region_start: before.region_start,
        field: field.clone(),
        program_type_id: frame.program.type_id,
        field_number: field.number,
        field_position: before.field_position,
        wire: field.wire_type,
        encoded_range: field.encoded_range,
        resolved,
        oneof_arm: match resolved.payload() {
            ResolvedProtobufPayload::Declared(_) => None,
            ResolvedProtobufPayload::Oneof(arm) => Some(arm.tag),
        },
        slot_count: frame.slots.len(),
        last_slot_before: before.last_slot,
        seen_before: before.seen,
        slot_occurrences_before: before.slot_occurrences,
        par_components_before: before.par_components,
        par_locally_free_before: before.par_locally_free,
        payload_len: payload_region.map(|region| region.len()),
        payload_position: if matches!(action, FrameAction::Push(_)) {
            payload_region
                .and_then(|region| region.position_at(0).map(|position| position.physical))
        } else {
            None
        },
        cursor_before: before.cursor,
        prestate: MessageFieldPrestateWitness {
            cursor: before.cursor,
            last_slot: before.last_slot,
            seen: before.seen,
        },
        cursor_after: frame.cursor.stats(),
        last_slot_after: frame.slots.last_slot(),
        seen_after: frame
            .slots
            .seen_at(resolved.slot())
            .expect("resolved generated slot is in bounds"),
        slot_occurrences_after: frame.semantic_slots[resolved.slot()].len(),
        waiting_after,
        child_kind,
        child_type_id,
        child_region_start,
        child_region_len,
        map_request: match action {
            FrameAction::Push(request) if request.request_kind() == FrameRequestKind::Map => {
                Some(request.clone())
            }
            _ => None,
        },
    };
    if !source.matches_frame(frame, action) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "admission frame action disagrees with its source observation",
        ));
    }
    *receipt = Some(source);
    Ok(())
}

#[inline(never)]
fn message_field_step_into<'src>(
    frame: &mut MessageFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    cursor_before: IndexedCanonicalFieldCursorStats,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<AdmissionStepKind, CanonicalProtobufAdmissionError> {
    prepare_message_field(frame, stats, cursor_before, scratch)?;
    decode_message_field_into(frame, scratch)?;
    verify_message_field_into(frame, scratch)?;
    Ok(finish_message_field_step(scratch))
}

#[inline(never)]
fn prepare_message_field<'src>(
    frame: &mut MessageFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    cursor_before: IndexedCanonicalFieldCursorStats,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let field = scratch.field_input.as_ref().expect("indexed message field");
    stats.fields = stats
        .fields
        .checked_add(1)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let resolved = resolve_protobuf_field(frame.program, field.number)
        .ok_or_else(|| frame.field_error(field, "field is outside the generated schema"))?;
    verify_wire(frame, field, resolved)?;
    let slot = resolved.slot();
    let mut before = MessageFieldBefore {
        body: frame.body.clone(),
        region_start: frame.region_start,
        last_slot: frame.slots.last_slot(),
        seen: frame.slots.seen_at(slot).unwrap_or(false),
        slot_occurrences: frame.semantic_slots[slot].len(),
        par_components: frame.par_components,
        par_locally_free: frame.par_locally_free_range,
        field_position: frame.region_start,
        cursor: cursor_before,
    };
    frame
        .slots
        .admit(slot, resolved.repeated())
        .map_err(|error| {
            frame.field_error(field, match error {
                SlotAdmissionError::OutOfRange => "field is outside the generated schema",
                SlotAdmissionError::Decreasing => "field is outside canonical Prost slot order",
                SlotAdmissionError::Duplicate => "canonical singular or oneof field is duplicated",
            })
        })?;
    before.field_position = field_absolute_offset(frame, field)?;
    if frame.is_raw_par() && matches!(resolved.field().kind, RhoMatchFieldKind::MessageSequence(_))
    {
        frame.par_components = frame
            .par_components
            .checked_add(1)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    }
    scratch.field_resolved = Some(resolved);
    scratch.field_before = Some(before);
    Ok(())
}

#[inline(never)]
fn decode_message_field_into<'src>(
    frame: &mut MessageFrame<'src>,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    match scratch
        .field_resolved
        .expect("resolved indexed field")
        .payload()
    {
        ResolvedProtobufPayload::Oneof(_) => decode_oneof_field_into(frame, scratch),
        ResolvedProtobufPayload::Declared(_) => decode_declared_field_into(frame, scratch),
    }
}

#[inline(never)]
fn decode_oneof_field_into<'src>(
    frame: &mut MessageFrame<'src>,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let field = scratch.field_input.as_ref().expect("indexed message field");
    let resolved = scratch.field_resolved.expect("resolved indexed field");
    let ResolvedProtobufPayload::Oneof(variant) = resolved.payload() else {
        unreachable!("oneof dispatch binds oneof payload")
    };
    let slot = resolved.slot();
    let field_position = scratch
        .field_before
        .as_ref()
        .expect("field prestate")
        .field_position;
    match variant.payload {
        RhoMatchPayloadKind::Message(type_id) => {
            let child = child_program(frame, field, type_id)?;
            let body = length_delimited(frame, field)?;
            frame.waiting = Some(MessageWait::Child {
                slot,
                variant: Some(variant.name),
                body: body.clone(),
                field_position,
            });
            scratch.field_action = Some(FrameAction::Push(FrameRequest::Message {
                region: body,
                program: child,
            }));
        }
        _ => {
            validate_scalar(frame, field, variant.protobuf_kind, ScalarPresence::Oneof)?;
            record_scalar(
                frame,
                field,
                slot,
                Some(variant.name),
                variant.protobuf_kind,
            )?;
            scratch.field_action = Some(FrameAction::Continue);
        }
    }
    Ok(())
}

#[inline(never)]
fn decode_declared_field_into<'src>(
    frame: &mut MessageFrame<'src>,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let field = scratch.field_input.as_ref().expect("indexed message field");
    let resolved = scratch.field_resolved.expect("resolved indexed field");
    let ResolvedProtobufPayload::Declared(kind) = resolved.payload() else {
        unreachable!("declared dispatch binds declared payload")
    };
    let slot = resolved.slot();
    let field_position = scratch
        .field_before
        .as_ref()
        .expect("field prestate")
        .field_position;
    match resolved.field().kind {
        RhoMatchFieldKind::Bool
        | RhoMatchFieldKind::I32
        | RhoMatchFieldKind::U32
        | RhoMatchFieldKind::I64
        | RhoMatchFieldKind::U64
        | RhoMatchFieldKind::Bytes
        | RhoMatchFieldKind::String => {
            validate_scalar(frame, field, kind, ScalarPresence::Plain)?;
            record_scalar(frame, field, slot, None, kind)?;
            scratch.field_action = Some(FrameAction::Continue);
        }
        RhoMatchFieldKind::StringSequence | RhoMatchFieldKind::BytesSequence => {
            validate_scalar(frame, field, kind, ScalarPresence::Repeated)?;
            record_scalar(frame, field, slot, None, kind)?;
            scratch.field_action = Some(FrameAction::Continue);
        }
        RhoMatchFieldKind::OptionalMessage(type_id)
        | RhoMatchFieldKind::MessageSequence(type_id) => {
            let child = child_program(frame, field, type_id)?;
            let body = length_delimited(frame, field)?;
            frame.waiting = Some(MessageWait::Child {
                slot,
                variant: None,
                body: body.clone(),
                field_position,
            });
            scratch.field_action = Some(FrameAction::Push(FrameRequest::Message {
                region: body,
                program: child,
            }));
        }
        RhoMatchFieldKind::StringMessageMap(type_id) => {
            let body = length_delimited(frame, field)?;
            frame.waiting = Some(MessageWait::Map {
                slot,
                field_position,
            });
            scratch.field_action = Some(FrameAction::Push(FrameRequest::Map {
                owner: frame.program,
                field: field.number,
                value_type_id: type_id,
                region: body,
            }));
        }
        RhoMatchFieldKind::PathMap => {
            let body = length_delimited(frame, field)?;
            frame.waiting = Some(MessageWait::EPathMap {
                slot,
                field_position,
            });
            scratch.field_action = Some(FrameAction::Push(FrameRequest::EPathMap {
                owner: frame.program,
                field: field.number,
                region: body,
            }));
        }
        RhoMatchFieldKind::Oneof(_) => {
            return Err(frame.field_error(field, "unresolved oneof marker reached admission"));
        }
        RhoMatchFieldKind::ParBag => {
            return Err(frame.field_error(
                field,
                "native ParBag field appeared in a protobuf wire program",
            ));
        }
    }
    Ok(())
}

#[inline(never)]
fn verify_message_field_into<'src>(
    frame: &MessageFrame<'src>,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    verify_message_field_action(
        frame,
        scratch.field_input.as_ref().expect("indexed message field"),
        scratch.field_resolved.expect("resolved indexed field"),
        scratch.field_before.take().expect("message field prestate"),
        scratch.field_action.as_ref().expect("decoded field action"),
        &mut scratch.field,
    )
}

#[inline(never)]
fn finish_message_field_step<'src>(scratch: &mut AdmissionScratch<'src>) -> AdmissionStepKind {
    match scratch.field_action.take().expect("verified field action") {
        FrameAction::Continue => AdmissionStepKind::MessageContinue,
        FrameAction::Push(request) => {
            scratch.request = Some(request);
            AdmissionStepKind::MessagePush
        }
        FrameAction::Complete(_) => unreachable!("message field produces no complete action"),
    }
}

#[cfg(test)]
fn message_field_step<'src>(
    frame: &mut MessageFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    field: IndexedCanonicalField<'src>,
    cursor_before: IndexedCanonicalFieldCursorStats,
) -> Result<FrameStep<'src>, CanonicalProtobufAdmissionError> {
    let mut scratch = AdmissionScratch::allocate();
    scratch.field_input = Some(field);
    let kind = message_field_step_into(frame, stats, cursor_before, &mut scratch)?;
    Ok(scratch.take_test_step(kind))
}

fn map_step_with_source<'src>(
    frame: &mut MapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<(FrameAction<'src>, MapStepSource<'src>), CanonicalProtobufAdmissionError> {
    if frame.waiting_value {
        return Err(frame.schema_error("map entry resumed before its value returned"));
    }
    let cursor_last_field_before = frame.cursor.last_field_number();
    let cursor_before = frame.cursor.stats();
    let field = frame
        .cursor
        .next_field()
        .map_err(|source| framing_error(frame.owner.type_name, frame.region_start, source))?;
    let Some(field) = field else {
        #[cfg(test)]
        rpb3_root_finish_trace::record_map_eof(frame);
        let cursor_stats = frame.cursor.stats();
        stats.protobuf_bytes_read = stats
            .protobuf_bytes_read
            .checked_add(cursor_stats.bytes_read)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        stats.protobuf_bytes_traversed = stats
            .protobuf_bytes_traversed
            .checked_add(cursor_stats.bytes_traversed)
            .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
        let source = MapEofObservation::from_frame(frame);
        let action = FrameAction::Complete(FrameSummary::Map(frame.finish()?));
        if !source.matches_frame(frame, &action) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "map completion disagrees with its source observation",
            ));
        }
        #[cfg(test)]
        if let FrameAction::Complete(FrameSummary::Map(summary)) = &action {
            rpb3_root_finish_trace::record_map_complete(frame, summary);
        }
        return Ok((action, MapStepSource::Eof(source)));
    };
    stats.fields = stats
        .fields
        .checked_add(1)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let field_position = field.field_start.physical;
    let owner_before = frame.owner;
    let body_before = frame.region.clone();
    let outer_field_before = frame.outer_field;
    let value_type_id_before = frame.value_type_id;
    let region_start_before = frame.region_start;
    let key_before = frame.key.is_some();
    let key_position_before = frame.key_position;
    let saw_value_before = frame.saw_value;
    let value_position_before = frame.value_position;
    let value_absent_before = frame.value.is_none();
    let waiting_before = frame.waiting_value;
    let (payload_len, payload_position) = match &field.value {
        IndexedCanonicalFieldValue::LengthDelimited(value) => (
            value.len(),
            value
                .position_at(0)
                .map(|position| position.physical)
                .unwrap_or(field_position),
        ),
        _ => (0, field_position),
    };
    let action = match field.number {
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
    }?;
    let source = MapFieldObservation {
        owner: owner_before,
        outer_field: outer_field_before,
        value_type_id: value_type_id_before,
        body: body_before,
        region_start: region_start_before,
        field: field.clone(),
        key_before,
        key_position_before,
        saw_value_before,
        value_position_before,
        value_absent_before,
        waiting_before,
        field_number: field.number,
        wire: field.wire_type,
        encoded_range: field.encoded_range.clone(),
        field_position,
        payload_len,
        payload_position,
        cursor_last_field_before,
        cursor_before,
        cursor_after: frame.cursor.stats(),
    };
    if !source.matches_frame(frame, &action) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "map field action disagrees with its source observation",
        ));
    }
    Ok((action, MapStepSource::Field(source)))
}

#[cfg(test)]
fn map_step<'src>(
    frame: &mut MapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
    map_step_with_source(frame, stats).map(|(action, _)| action)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EPathMapLoopWaitSource {
    None,
    MapValue,
    Obligation {
        id: CanonicalPathObligationId,
        kind: CanonicalPathObligationKind,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EPathMapLoopTickKind {
    AcceptedForward {
        start: usize,
        width: usize,
    },
    AcceptedUnwind {
        start: usize,
        width: usize,
    },
    ScalarEdgeNoChild {
        byte: u8,
    },
    ScalarLeave,
    ScalarValueNoBody {
        has_value: bool,
        reducer_stable: Option<bool>,
        root_stable: bool,
    },
    PushEdge {
        byte: u8,
    },
    PushValue {
        has_value: bool,
        reducer_stable: Option<bool>,
        root_stable: bool,
        body_range: CanonicalByteRange,
    },
    Complete,
}

/// One constant-size view of the live EPM loop boundary. No loop history or
/// payload bytes are retained. The indexed cursor supplies the absolute event
/// index; ACT's source position identifies the active validated-plan record.
#[cfg_attr(test, derive(Clone, Copy))]
struct EPathMapLoopStateSource {
    owner: &'static RhoMatchProgram,
    field: u32,
    region_start: CanonicalByteProvenance,
    snapshot_len: usize,
    cursor: crate::rust::epathmap_trie_codec::IndexedCanonicalEpm1ActiveSourceImage,
    topology:
        Option<rspace_plus_plus::rspace::pathmap_codec::CanonicalCompactTrieCursorSourcePosition>,
    path: super::canonical_path_admission::CanonicalPathBatchSourceShape,
    root_gate: super::canonical_path_admission::CanonicalPathBatchGateSource,
    obligation: Option<CanonicalPathObligationProgress>,
    last_source: CanonicalPathSource,
    waiting: EPathMapLoopWaitSource,
    waiting_child_len: Option<usize>,
    entries_canonical_path_stable: bool,
}

impl EPathMapLoopStateSource {
    fn matches_source(&self, other: &Self) -> bool {
        std::ptr::eq(self.owner, other.owner)
            && self.field == other.field
            && self.region_start == other.region_start
            && self.snapshot_len == other.snapshot_len
            && self.cursor == other.cursor
            && self.topology == other.topology
            && self.path == other.path
            && self.root_gate == other.root_gate
            && self.obligation == other.obligation
            && self.last_source == other.last_source
            && self.waiting == other.waiting
            && self.waiting_child_len == other.waiting_child_len
            && self.entries_canonical_path_stable == other.entries_canonical_path_stable
    }
}

struct EPathMapScalarEdgeNoChildReceipt<'a> {
    before: &'a EPathMapLoopStateSource,
    after: EPathMapLoopStateSource,
    before_path: CanonicalPathBatchHandleSource,
    after_path: CanonicalPathBatchHandleSource,
    byte: u8,
    provenance: CanonicalByteProvenance,
    no_child: bool,
    no_map_value_receipt: bool,
    #[cfg(test)]
    parser_before: Option<CanonicalPathParserSourceImage>,
    #[cfg(test)]
    parser_after: Option<CanonicalPathParserSourceImage>,
}

impl EPathMapScalarEdgeNoChildReceipt<'_> {
    fn matches_frame(&self, frame: &EPathMapFrame<'_>) -> bool {
        let source = CanonicalPathSource::in_source(self.provenance.source, self.provenance.offset);
        self.no_child
            && self.no_map_value_receipt
            && self.before.matches_successor(
                &self.after,
                EPathMapLoopTickKind::ScalarEdgeNoChild { byte: self.byte },
                source,
            )
            && self.after.matches_frame(frame)
            && self.before_path.state_id == self.before.path.state_id
            && self.before_path.depth == self.before.path.depth
            && self.after_path.state_id == self.after.path.state_id
            && self.after_path.depth == self.after.path.depth
            && self.before_path.fault_free
            && self.after_path.fault_free
            && self.after_path.last_source == Some(source)
            && frame.waiting.is_none()
            && frame.waiting_child.is_none()
            && {
                #[cfg(test)]
                {
                    match (&self.parser_before, &self.parser_after) {
                        (Some(before), Some(after)) => {
                            let prior = &before.path_bytes;
                            let next = &after.path_bytes;
                            prior.len() == self.before_path.depth
                                && next.len() == self.after_path.depth
                                && next.len() == prior.len() + 1
                                && next.starts_with(prior)
                                && next.last()
                                    == Some(&LocatedPathByte::in_source(
                                        self.byte,
                                        self.provenance.source,
                                        self.provenance.offset,
                                    ))
                                && before.root_gate == self.before.root_gate
                                && after.root_gate == self.after.root_gate
                        }
                        (None, None) => true,
                        _ => false,
                    }
                }
                #[cfg(not(test))]
                {
                    true
                }
            }
    }
}

impl EPathMapLoopStateSource {
    fn capture(frame: &EPathMapFrame<'_>) -> Result<Self, CanonicalProtobufAdmissionError> {
        let cursor = frame.cursor.active_source_image().ok_or(
            CanonicalProtobufAdmissionError::CutoverInvariant(
                "EPM1 loop boundary has no active cursor source image",
            ),
        )?;
        if cursor.snapshot_len != frame.snapshot.len() || cursor.mode != frame.mode {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "EPM1 loop cursor disagrees with its owned snapshot",
            ));
        }
        let waiting = match frame.waiting.as_ref() {
            None => EPathMapLoopWaitSource::None,
            Some(EPathMapWait::MapValue) => EPathMapLoopWaitSource::MapValue,
            Some(EPathMapWait::Obligation(wait)) => EPathMapLoopWaitSource::Obligation {
                id: wait.id,
                kind: wait.kind,
            },
        };
        Ok(Self {
            owner: frame.owner,
            field: frame.field,
            region_start: frame.region_start,
            snapshot_len: frame.snapshot.len(),
            cursor,
            topology: frame.cursor.topology_source_position(),
            path: frame.path.batch_source_shape(),
            root_gate: frame.path.value_root_gate_source(),
            obligation: frame.path.active_obligation(),
            last_source: frame.last_source,
            waiting,
            waiting_child_len: frame.waiting_child.as_ref().map(AdmissionRegion::len),
            entries_canonical_path_stable: frame.entries_canonical_path_stable,
        })
    }

    fn matches_frame(&self, frame: &EPathMapFrame<'_>) -> bool {
        std::ptr::eq(self.owner, frame.owner)
            && self.field == frame.field
            && self.region_start == frame.region_start
            && self.snapshot_len == frame.snapshot.len()
            && frame.cursor.active_source_image() == Some(self.cursor)
            && self.topology == frame.cursor.topology_source_position()
            && self.path == frame.path.batch_source_shape()
            && self.root_gate == frame.path.value_root_gate_source()
            && self.obligation == frame.path.active_obligation()
            && self.last_source == frame.last_source
            && self.waiting
                == match frame.waiting.as_ref() {
                    None => EPathMapLoopWaitSource::None,
                    Some(EPathMapWait::MapValue) => EPathMapLoopWaitSource::MapValue,
                    Some(EPathMapWait::Obligation(wait)) => EPathMapLoopWaitSource::Obligation {
                        id: wait.id,
                        kind: wait.kind,
                    },
                }
            && self.waiting_child_len == frame.waiting_child.as_ref().map(AdmissionRegion::len)
            && self.entries_canonical_path_stable == frame.entries_canonical_path_stable
    }

    fn matches_successor(
        &self,
        after: &Self,
        kind: EPathMapLoopTickKind,
        physical_source: CanonicalPathSource,
    ) -> bool {
        let (events, map_values, waiting) = match kind {
            EPathMapLoopTickKind::AcceptedForward { width, .. } => {
                let Some(events) = width.checked_mul(2) else {
                    return false;
                };
                (events, 0, EPathMapLoopWaitSource::None)
            }
            EPathMapLoopTickKind::AcceptedUnwind { width, .. } => {
                (width, 0, EPathMapLoopWaitSource::None)
            }
            EPathMapLoopTickKind::ScalarEdgeNoChild { .. }
            | EPathMapLoopTickKind::ScalarLeave
            | EPathMapLoopTickKind::ScalarValueNoBody { .. } => {
                (1, 0, EPathMapLoopWaitSource::None)
            }
            EPathMapLoopTickKind::PushEdge { .. } => {
                let EPathMapLoopWaitSource::Obligation { id, kind } = after.waiting else {
                    return false;
                };
                (1, 0, EPathMapLoopWaitSource::Obligation { id, kind })
            }
            EPathMapLoopTickKind::PushValue { has_value, .. } => (
                1,
                usize::from(has_value && after.cursor.mode == EPathMapMode::Map),
                EPathMapLoopWaitSource::MapValue,
            ),
            EPathMapLoopTickKind::Complete => return false,
        };
        std::ptr::eq(self.owner, after.owner)
            && self.field == after.field
            && self.region_start == after.region_start
            && self.snapshot_len == after.snapshot_len
            && self.cursor.mode == after.cursor.mode
            && self.cursor.declared_value_count == after.cursor.declared_value_count
            && self.cursor.snapshot_len == after.cursor.snapshot_len
            && after.cursor.snapshot_len == after.snapshot_len
            && self.cursor.event_index.checked_add(events) == Some(after.cursor.event_index)
            && self.cursor.map_values_seen.checked_add(map_values)
                == Some(after.cursor.map_values_seen)
            && self.cursor.value_table_offset <= after.cursor.value_table_offset
            && after.cursor.value_table_offset <= after.cursor.snapshot_len
            && self.waiting == EPathMapLoopWaitSource::None
            && self.waiting_child_len.is_none()
            && after.waiting == waiting
            && (matches!(
                kind,
                EPathMapLoopTickKind::PushEdge { .. } | EPathMapLoopTickKind::PushValue { .. }
            ) == after.waiting_child_len.is_some())
            && after.last_source == physical_source
            && match kind {
                EPathMapLoopTickKind::AcceptedForward { start, width } => {
                    self.path.matches_forward(after.path, width)
                        && self.cursor.value_table_offset == after.cursor.value_table_offset
                        && self.entries_canonical_path_stable == after.entries_canonical_path_stable
                        && start.checked_add(width).is_some()
                }
                EPathMapLoopTickKind::AcceptedUnwind { start, width } => {
                    self.path.matches_unwind(after.path, width)
                        && self.cursor.value_table_offset == after.cursor.value_table_offset
                        && self.entries_canonical_path_stable == after.entries_canonical_path_stable
                        && start.checked_add(width).is_some()
                }
                EPathMapLoopTickKind::ScalarEdgeNoChild { .. } => {
                    self.cursor.value_table_offset == after.cursor.value_table_offset
                        && self.path.matches_scalar_edge(after.path)
                        && self.entries_canonical_path_stable == after.entries_canonical_path_stable
                }
                EPathMapLoopTickKind::ScalarLeave => {
                    self.cursor.value_table_offset == after.cursor.value_table_offset
                        && self.path.matches_scalar_leave(after.path)
                        && self.entries_canonical_path_stable == after.entries_canonical_path_stable
                }
                EPathMapLoopTickKind::PushEdge { .. } => {
                    self.cursor.value_table_offset == after.cursor.value_table_offset
                        && self.path.matches_scalar_edge(after.path)
                        && self.entries_canonical_path_stable == after.entries_canonical_path_stable
                        && after.obligation.is_some_and(|progress| {
                            progress.awaiting_resolution
                                && progress.remaining == 0
                                && matches!(after.waiting,
                                    EPathMapLoopWaitSource::Obligation { id, kind }
                                    if id == progress.obligation.id
                                        && kind == progress.obligation.kind)
                        })
                }
                EPathMapLoopTickKind::ScalarValueNoBody {
                    has_value,
                    reducer_stable,
                    root_stable,
                } => {
                    self.cursor.value_table_offset == after.cursor.value_table_offset
                        && self.path == after.path
                        && self.root_gate == after.root_gate
                        && root_stable == self.root_gate.stable
                        && self.obligation == after.obligation
                        && reducer_stable == has_value.then_some(root_stable)
                        && after.entries_canonical_path_stable
                            == (self.entries_canonical_path_stable
                                && reducer_stable.unwrap_or(true))
                }
                EPathMapLoopTickKind::PushValue {
                    has_value,
                    reducer_stable,
                    root_stable,
                    body_range,
                } => {
                    self.path == after.path
                        && self.root_gate == after.root_gate
                        && root_stable == self.root_gate.stable
                        && self.obligation == after.obligation
                        && after.obligation.is_none()
                        && self.cursor.mode == EPathMapMode::Map
                        && has_value
                        && self.cursor.value_table_offset < body_range.start
                        && body_range.start <= body_range.end
                        && body_range.end == after.cursor.value_table_offset
                        && after.waiting_child_len == Some(body_range.end - body_range.start)
                        && reducer_stable == has_value.then_some(root_stable)
                        && after.entries_canonical_path_stable
                            == (self.entries_canonical_path_stable
                                && reducer_stable.unwrap_or(true))
                }
                EPathMapLoopTickKind::Complete => false,
            }
    }

    fn matches_complete_stop(
        &self,
        kind: EPathMapLoopTickKind,
        frame: &EPathMapFrame<'_>,
        terminal: IndexedCanonicalEpm1TerminalSourceImage,
    ) -> bool {
        kind == EPathMapLoopTickKind::Complete
            && std::ptr::eq(self.owner, frame.owner)
            && self.field == frame.field
            && self.region_start == frame.region_start
            && self.snapshot_len == frame.snapshot.len()
            && self.cursor.mode == terminal.mode
            && self.cursor.declared_value_count == terminal.declared_value_count
            && self.cursor.map_values_seen == terminal.map_values_seen
            && self.cursor.event_index == terminal.event_index
            && self.cursor.value_table_offset == terminal.value_table_offset
            && self.cursor.snapshot_len == terminal.snapshot_len
            && self.waiting == EPathMapLoopWaitSource::None
            && self.waiting_child_len.is_none()
            && self.obligation.is_none()
            && self.last_source == frame.last_source
            && self.entries_canonical_path_stable == frame.entries_canonical_path_stable
            && terminal.terminal_checks == 1
    }
}

fn try_epathmap_batch(
    frame: &mut EPathMapFrame<'_>,
    loop_before: &EPathMapLoopStateSource,
) -> Result<bool, CanonicalProtobufAdmissionError> {
    #[cfg(test)]
    if standalone_path_admission::scalar_epm_transport() {
        return Ok(false);
    }
    let line_work = frame.cursor.active_line_work_source();
    let cursor_before = frame.cursor.active_source_image();
    let topology_before = frame.cursor.topology_source_position();
    let path_before = frame.path.batch_source_shape();
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
    let mut forward_receipt = None;
    let accepted = match direction {
        CanonicalCompactTrieLineDirection::Forward => {
            forward_receipt = frame
                .path
                .try_forward_source_with_receipt(prepared.source_range())
                .map_err(|_| CanonicalProtobufAdmissionError::SizeOverflow)?;
            forward_receipt.is_some()
        }
        CanonicalCompactTrieLineDirection::Unwind => frame.path.try_leave_run(width).is_some(),
    };
    if !accepted {
        return Ok(false);
    }
    let path_after = frame.path.batch_source_shape();
    let path_matches = match direction {
        CanonicalCompactTrieLineDirection::Forward => {
            path_before.matches_forward(path_after, width)
        }
        CanonicalCompactTrieLineDirection::Unwind => path_before.matches_unwind(path_after, width),
    };
    if !path_matches {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 batch path source shape disagrees with scalar expansion",
        ));
    }
    let cursor_before = cursor_before.ok_or(CanonicalProtobufAdmissionError::CutoverInvariant(
        "EPM1 batch has no active cursor source image",
    ))?;
    let expanded_events = match direction {
        CanonicalCompactTrieLineDirection::Forward => width.checked_mul(2),
        CanonicalCompactTrieLineDirection::Unwind => Some(width),
    }
    .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let expected_event_index = cursor_before
        .event_index
        .checked_add(expanded_events)
        .ok_or(CanonicalProtobufAdmissionError::SizeOverflow)?;
    let source_range = prepared.source_range();
    let (source, publication) = prepared.commit();
    if let Some(receipt) = forward_receipt.as_ref() {
        let view =
            frame
                .source_view
                .as_ref()
                .ok_or(CanonicalProtobufAdmissionError::CutoverInvariant(
                    "EPM1 Forward batch has no registered source view",
                ))?;
        if !receipt.matches_batch(
            &frame.path,
            view,
            source_range.clone(),
            path_before,
            path_after,
            CanonicalPathSource::in_source(source.source, source.offset),
        ) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "EPM1 Forward receipt disagrees with its registered source and reducer",
            ));
        }
    }
    let cursor_after = frame.cursor.active_source_image().ok_or(
        CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 batch lost its active cursor source image",
        ),
    )?;
    let topology_after = frame.cursor.topology_source_position();
    if cursor_after.event_index != expected_event_index
        || cursor_after.mode != cursor_before.mode
        || cursor_after.declared_value_count != cursor_before.declared_value_count
        || cursor_after.map_values_seen != cursor_before.map_values_seen
        || cursor_after.value_table_offset != cursor_before.value_table_offset
        || cursor_after.snapshot_len != cursor_before.snapshot_len
        || !topology_before
            .zip(topology_after)
            .is_some_and(|(before, after)| publication.matches_scalar_schedule(before, after))
    {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 batch cursor source image disagrees with scalar expansion",
        ));
    }
    frame.last_source = CanonicalPathSource::in_source(source.source, source.offset);
    let loop_after = EPathMapLoopStateSource::capture(frame)?;
    let kind = match direction {
        CanonicalCompactTrieLineDirection::Forward => EPathMapLoopTickKind::AcceptedForward {
            start: source_range.start,
            width,
        },
        CanonicalCompactTrieLineDirection::Unwind => EPathMapLoopTickKind::AcceptedUnwind {
            start: source_range.start,
            width,
        },
    };
    let line_matches = line_work.is_some_and(|work| {
        loop_before.topology == Some(work.position)
            && work.event_index == loop_before.cursor.event_index
            && work.direction == direction
            && work.admissible_source_width >= width
            && match direction {
                CanonicalCompactTrieLineDirection::Forward => {
                    work.source_range.start == source_range.start
                        && source_range.end <= work.source_range.end
                }
                CanonicalCompactTrieLineDirection::Unwind => {
                    work.source_range.end == source_range.end
                        && work.source_range.start <= source_range.start
                }
            }
    });
    if !line_matches
        || !loop_before.matches_successor(&loop_after, kind, frame.last_source)
        || !loop_after.matches_frame(frame)
    {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 accepted batch disagrees with its loop source observation",
        ));
    }
    #[cfg(test)]
    standalone_path_admission::record_epm_batch(direction, width);
    #[cfg(test)]
    rpb3_root_finish_trace::record_epathmap_event(
        frame,
        match direction {
            CanonicalCompactTrieLineDirection::Forward => {
                rpb3_root_finish_trace::EPathMapEventKind::Forward {
                    width,
                    source_range,
                }
            }
            CanonicalCompactTrieLineDirection::Unwind => {
                rpb3_root_finish_trace::EPathMapEventKind::Unwind {
                    width,
                    source_range,
                }
            }
        },
        source,
    );
    Ok(true)
}

fn check_epathmap_loop_tick(
    frame: &EPathMapFrame<'_>,
    before: &EPathMapLoopStateSource,
    kind: EPathMapLoopTickKind,
    physical_source: CanonicalPathSource,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let after = EPathMapLoopStateSource::capture(frame)?;
    if !before.matches_successor(&after, kind, physical_source) || !after.matches_frame(frame) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 scalar tick disagrees with its loop source observation",
        ));
    }
    Ok(())
}

fn check_epathmap_scalar_edge_no_child(
    frame: &EPathMapFrame<'_>,
    before: &EPathMapLoopStateSource,
    before_path: CanonicalPathBatchHandleSource,
    byte: u8,
    provenance: CanonicalByteProvenance,
    no_child: bool,
    no_map_value_receipt: bool,
    #[cfg(test)] parser_before: Option<CanonicalPathParserSourceImage>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let after = EPathMapLoopStateSource::capture(frame)?;
    let after_path = frame.path.batch_current_source();
    #[cfg(test)]
    let parser_after = parser_before
        .as_ref()
        .map(|_| frame.path.parser_source_image());
    let receipt = EPathMapScalarEdgeNoChildReceipt {
        before,
        after,
        before_path,
        after_path,
        byte,
        provenance,
        no_child,
        no_map_value_receipt,
        #[cfg(test)]
        parser_before,
        #[cfg(test)]
        parser_after,
    };
    if !receipt.matches_frame(frame) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 scalar Edge without child disagrees with its source receipt",
        ));
    }
    #[cfg(test)]
    if rpb3_root_finish_trace::is_recording_scalar_edge_receipts() {
        rpb3_root_finish_trace::record_epathmap_scalar_edge_no_child(&receipt);
    }
    Ok(())
}

// Keep terminal observation locals out of the scalar event loop's stack frame.
#[inline(never)]
fn epathmap_terminal_step<'src>(
    frame: &mut EPathMapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    loop_before: EPathMapLoopStateSource,
) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
    frame
        .path
        .finish(frame.last_source)
        .map_err(|source| frame.path_error(source))?;
    let cursor_source = frame.cursor.terminal_source_image().ok_or(
        CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 terminal cursor has no successful indexed source image",
        ),
    )?;
    if !loop_before.matches_complete_stop(EPathMapLoopTickKind::Complete, frame, cursor_source) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 complete stop disagrees with its loop source observation",
        ));
    }
    let cursor_stats = frame.cursor.stats();
    let protobuf_read_before = stats.protobuf_bytes_read;
    let protobuf_traversed_before = stats.protobuf_bytes_traversed;
    let events_before = stats.epathmap_events;
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
    let source = EPathMapTerminalSource {
        snapshot: frame.snapshot.clone(),
        region_start: frame.region_start,
        end: frame.snapshot.end_position().physical,
        cursor: cursor_stats,
        cursor_source,
        last_source: frame.last_source,
        mode: frame.mode,
        entries_canonical_path_stable: frame.entries_canonical_path_stable,
        protobuf_read_before,
        protobuf_traversed_before,
        events_before,
    };
    let action = FrameAction::Complete(FrameSummary::EPathMap(EPathMapSummary {
        mode: frame.mode,
        snapshot: frame.snapshot.clone(),
        entries_canonical_path_stable: frame.entries_canonical_path_stable,
    }));
    let FrameAction::Complete(returned) = &action else {
        unreachable!()
    };
    if !source.matches_complete(frame, returned, stats) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 terminal return disagrees with its indexed source observation",
        ));
    }
    frame.terminal_source = Some(Box::new(source));
    Ok(action)
}

#[derive(Clone, Copy)]
enum EPathMapFetchedKind {
    Edge,
    Leave,
    Value,
}

#[inline(never)]
fn epathmap_step_into<'src>(
    frame: &mut EPathMapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<AdmissionStepKind, CanonicalProtobufAdmissionError> {
    if frame.waiting.is_some() || frame.waiting_child.is_some() {
        return Err(CanonicalProtobufAdmissionError::Schema {
            type_name: frame.owner.type_name,
            field: frame.field,
            position: frame.region_start,
            detail: "EPM1 cursor resumed before its value returned",
        });
    }
    if frame.terminal_source.is_some() {
        return Err(CanonicalProtobufAdmissionError::Schema {
            type_name: frame.owner.type_name,
            field: frame.field,
            position: frame.region_start,
            detail: "EPM1 cursor resumed after terminal completion",
        });
    }
    scratch.epm_step_start = Some(EPathMapLoopStateSource::capture(frame)?);
    loop {
        scratch.epm_before = Some(EPathMapLoopStateSource::capture(frame)?);
        if epathmap_batch_phase(frame, scratch)? {
            continue;
        }
        if !epathmap_fetch_phase(frame, scratch)? {
            return epathmap_terminal_phase(frame, stats, scratch);
        }
        let fetched = match &scratch
            .epm_event
            .as_ref()
            .expect("fetched EPM1 event")
            .event
        {
            IndexedCanonicalEpm1TopologyEvent::Edge { .. } => EPathMapFetchedKind::Edge,
            IndexedCanonicalEpm1TopologyEvent::Leave { .. } => EPathMapFetchedKind::Leave,
            IndexedCanonicalEpm1TopologyEvent::Value { .. } => EPathMapFetchedKind::Value,
        };
        let stop = match fetched {
            EPathMapFetchedKind::Edge => epathmap_edge_phase(frame, stats, scratch)?,
            EPathMapFetchedKind::Leave => epathmap_leave_phase(frame, scratch)?,
            EPathMapFetchedKind::Value => epathmap_value_phase(frame, scratch)?,
        };
        if let Some(kind) = stop {
            return Ok(kind);
        }
    }
}

#[cfg(test)]
fn epathmap_step<'src>(
    frame: &mut EPathMapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<FrameAction<'src>, CanonicalProtobufAdmissionError> {
    let mut scratch = AdmissionScratch::allocate();
    let kind = epathmap_step_into(frame, stats, &mut scratch)?;
    let FrameStep::Other { action, .. } = scratch.take_test_step(kind) else {
        unreachable!("EPM1 step produces a nonmessage action")
    };
    Ok(action)
}

#[inline(never)]
fn epathmap_batch_phase<'src>(
    frame: &mut EPathMapFrame<'src>,
    scratch: &AdmissionScratch<'src>,
) -> Result<bool, CanonicalProtobufAdmissionError> {
    let before = scratch.epm_before.as_ref().expect("EPM1 loop source");
    if try_epathmap_batch(frame, before)? {
        return Ok(true);
    }
    if !before.matches_frame(frame) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 declined batch changed its loop source boundary",
        ));
    }
    Ok(false)
}

#[inline(never)]
fn epathmap_fetch_phase<'src>(
    frame: &mut EPathMapFrame<'src>,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<bool, CanonicalProtobufAdmissionError> {
    scratch.epm_event = frame.cursor.next_event_with_source().map_err(|source| {
        CanonicalProtobufAdmissionError::EPathMap {
            type_name: frame.owner.type_name,
            field: frame.field,
            position: frame.region_start,
            source,
        }
    })?;
    Ok(scratch.epm_event.is_some())
}

#[inline(never)]
fn epathmap_terminal_phase<'src>(
    frame: &mut EPathMapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<AdmissionStepKind, CanonicalProtobufAdmissionError> {
    let before = scratch.epm_before.take().expect("EPM1 loop source");
    #[cfg(test)]
    {
        scratch.epm_terminal_before = Some(before);
    }
    Ok(scratch.store_other(epathmap_terminal_step(frame, stats, before)?, None))
}

#[inline(never)]
fn epathmap_edge_phase<'src>(
    frame: &mut EPathMapFrame<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<Option<AdmissionStepKind>, CanonicalProtobufAdmissionError> {
    let source_event = scratch.epm_event.take().expect("fetched EPM1 event");
    let IndexedCanonicalEpm1TopologyEvent::Edge { byte, provenance } = source_event.event else {
        unreachable!("edge dispatch binds edge event")
    };
    let loop_before = scratch.epm_before.as_ref().expect("EPM1 loop source");
    let before_path = frame.path.batch_current_source();
    #[cfg(test)]
    let parser_before = rpb3_root_finish_trace::is_recording_scalar_edge_receipts()
        .then(|| frame.path.parser_source_image());
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
    let completed = frame
        .path
        .retained_completed_obligation()
        .map_err(|source| frame.path_error(source))?;
    let no_child = completed.is_none();
    if let Some(completed) = completed {
        let (waiting, child) = obligation_frame(completed, frame.owner, frame.field, stats)?;
        frame.waiting = Some(EPathMapWait::Obligation(waiting));
        frame.waiting_child = Some(child.region().clone());
        if !frame.matches_push_request(&child) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "EPM1 edge stop disagrees with its child request",
            ));
        }
        check_epathmap_loop_tick(
            frame,
            loop_before,
            EPathMapLoopTickKind::PushEdge { byte },
            source,
        )?;
        #[cfg(test)]
        rpb3_root_finish_trace::record_epathmap_event(
            frame,
            rpb3_root_finish_trace::EPathMapEventKind::Edge(byte),
            provenance,
        );
        scratch.request = Some(child);
        return Ok(Some(AdmissionStepKind::OtherPush));
    }
    check_epathmap_scalar_edge_no_child(
        frame,
        loop_before,
        before_path,
        byte,
        provenance,
        no_child,
        source_event.map_value_receipt.is_none(),
        #[cfg(test)]
        parser_before,
    )?;
    #[cfg(test)]
    rpb3_root_finish_trace::record_epathmap_event(
        frame,
        rpb3_root_finish_trace::EPathMapEventKind::Edge(byte),
        provenance,
    );
    Ok(None)
}

#[inline(never)]
fn epathmap_leave_phase<'src>(
    frame: &mut EPathMapFrame<'src>,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<Option<AdmissionStepKind>, CanonicalProtobufAdmissionError> {
    let source_event = scratch.epm_event.take().expect("fetched EPM1 event");
    let IndexedCanonicalEpm1TopologyEvent::Leave { provenance } = source_event.event else {
        unreachable!("leave dispatch binds leave event")
    };
    let loop_before = scratch.epm_before.as_ref().expect("EPM1 loop source");
    let source = CanonicalPathSource::in_source(provenance.source, provenance.offset);
    frame.last_source = source;
    frame
        .path
        .leave(source)
        .map_err(|source| frame.path_error(source))?;
    check_epathmap_loop_tick(
        frame,
        loop_before,
        EPathMapLoopTickKind::ScalarLeave,
        source,
    )?;
    #[cfg(test)]
    rpb3_root_finish_trace::record_epathmap_event(
        frame,
        rpb3_root_finish_trace::EPathMapEventKind::Leave,
        provenance,
    );
    Ok(None)
}

#[inline(never)]
fn epathmap_value_phase<'src>(
    frame: &mut EPathMapFrame<'src>,
    scratch: &mut AdmissionScratch<'src>,
) -> Result<Option<AdmissionStepKind>, CanonicalProtobufAdmissionError> {
    let source_event = scratch.epm_event.take().expect("fetched EPM1 event");
    let IndexedCanonicalEpm1TopologyEvent::Value {
        has_value,
        value_body,
        provenance,
        value_body_range,
        source_offset,
    } = source_event.event
    else {
        unreachable!("value dispatch binds value event")
    };
    let loop_before = scratch.epm_before.as_ref().expect("EPM1 loop source");
    let source = CanonicalPathSource::in_source(provenance.source, provenance.offset);
    frame.last_source = source;
    let root_stable = frame.path.value_root_stability_source();
    let reducer_stable = if let Some(summary) = frame.path.value(has_value, source) {
        let summary = summary.map_err(|source| frame.path_error(source))?;
        frame.entries_canonical_path_stable &= summary.canonical_path_stable;
        Some(summary.canonical_path_stable)
    } else {
        None
    };
    if let Some(value) = value_body {
        let Some(range) = value_body_range.as_ref() else {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "EPM1 Map value body has no indexed source range",
            ));
        };
        if !source_event.map_value_receipt.is_some_and(|receipt| {
            receipt.act_source_offset == source_offset
                && receipt.act_source == provenance
                && receipt.body_range.start == range.start
                && receipt.body_range.end == range.end
                && receipt.body_len == value.len()
                && receipt.post_value_table_offset == range.end
        }) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "EPM1 Map value event disagrees with its cursor source receipt",
            ));
        }
        let body_range = CanonicalByteRange {
            start: range.start,
            end: range.end,
        };
        if !frame
            .snapshot
            .subview(body_range)
            .is_some_and(|body| value.same_view(&body))
        {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "EPM1 Map value body disagrees with its indexed snapshot range",
            ));
        }
        frame.waiting = Some(EPathMapWait::MapValue);
        frame.waiting_child = Some(value.clone());
        let request = FrameRequest::Message {
            region: value,
            program: &PAR_PROTOBUF_REGION_PROGRAM,
        };
        if !frame.matches_push_request(&request) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "EPM1 value stop disagrees with its child request",
            ));
        }
        check_epathmap_loop_tick(
            frame,
            loop_before,
            EPathMapLoopTickKind::PushValue {
                has_value,
                reducer_stable,
                root_stable,
                body_range,
            },
            source,
        )?;
        #[cfg(test)]
        rpb3_root_finish_trace::record_epathmap_event(
            frame,
            rpb3_root_finish_trace::EPathMapEventKind::Value {
                has_value,
                value_body_range,
            },
            provenance,
        );
        scratch.request = Some(request);
        return Ok(Some(AdmissionStepKind::OtherPush));
    }
    if value_body_range.is_some() {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 bodyless value retained an indexed body range",
        ));
    }
    if source_event.map_value_receipt.is_some() {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "EPM1 bodyless value retained a Map source receipt",
        ));
    }
    check_epathmap_loop_tick(
        frame,
        loop_before,
        EPathMapLoopTickKind::ScalarValueNoBody {
            has_value,
            reducer_stable,
            root_stable,
        },
        source,
    )?;
    #[cfg(test)]
    rpb3_root_finish_trace::record_epathmap_event(
        frame,
        rpb3_root_finish_trace::EPathMapEventKind::Value {
            has_value,
            value_body_range,
        },
        provenance,
    );
    Ok(None)
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
) -> Result<FramePushObservation, CanonicalProtobufAdmissionError> {
    let mut request_slot = Some(request);
    let mut pushed = None;
    push_admission_frame_into(frames, &mut request_slot, stats, &mut pushed, None)?;
    Ok(pushed.expect("checked admission push observation"))
}

#[cfg(test)]
fn push_admission_frame_with_source<'src>(
    frames: &mut Vec<Frame<'src>>,
    request: FrameRequest<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<(FramePushObservation, FrameRequest<'src>), CanonicalProtobufAdmissionError> {
    let mut request_slot = Some(request);
    let mut pushed = None;
    let mut pushed_request = None;
    push_admission_frame_into(
        frames,
        &mut request_slot,
        stats,
        &mut pushed,
        Some(&mut pushed_request),
    )?;
    Ok((
        pushed.expect("checked admission push observation"),
        pushed_request.expect("checked admission push source"),
    ))
}

#[inline(never)]
fn push_admission_frame_into<'src>(
    frames: &mut Vec<Frame<'src>>,
    request_slot: &mut Option<FrameRequest<'src>>,
    stats: &mut CanonicalProtobufAdmissionStats,
    pushed_out: &mut Option<FramePushObservation>,
    source_out: Option<&mut Option<FrameRequest<'src>>>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let request = request_slot.take().expect("admission child request");
    let before_len = frames.len();
    let before_capacity = frames.capacity();
    let request_kind = request.request_kind();
    let request_source = request.clone();
    match request {
        FrameRequest::Message { region, program } => {
            FrameRequest::push_message(frames, stats, region, program)?;
        }
        FrameRequest::Map {
            region,
            owner,
            field,
            value_type_id,
        } => {
            FrameRequest::push_map(frames, stats, owner, field, value_type_id, region)?;
        }
        FrameRequest::EPathMap {
            region,
            owner,
            field,
        } => {
            let (region_start, cursor) = EPathMapFrame::prepare_epm1_cursor(owner, field, &region)?;
            EPathMapFrame::with_prepared_epm1_region(
                owner,
                field,
                region,
                region_start,
                cursor,
                |frame| {
                    increment_frame_count(&mut stats.epathmaps)?;
                    frames.push(Frame::EPathMap(frame));
                    Ok(())
                },
            )?;
        }
        FrameRequest::CanonicalPath(region) => {
            FrameRequest::push_canonical_path(frames, region);
        }
    }
    if !frames
        .last()
        .is_some_and(|frame| request_source.matches_pushed_frame(frame))
    {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "admission push changed the requested source region or descriptor",
        ));
    }
    let observation =
        FramePushObservation::after_push(request_kind, before_len, before_capacity, frames, stats)?;
    *pushed_out = Some(observation);
    if let Some(source_out) = source_out {
        *source_out = Some(request_source);
    }
    Ok(())
}

enum CompleteAdmissionOutcome<'src> {
    AcceptedMessage(MessageAcceptSourceReceipt<'src>),
    AcceptedOther,
    Root(FrameSummary<'src>),
}

// Every successful driver iteration emits exactly one borrowed, checked
// transition. A completion may contain both an EOF and a parent acceptance;
// the consumer projects those two logical effects from the same transition.
// Keeping this event local to the iteration avoids a second retained source
// model or an input-size-dependent production log.
enum CheckedAdmissionAction<'step, 'src> {
    MessageField {
        field: &'step MessageFieldSourceReceipt<'src>,
        pushed: Option<&'step FramePushObservation>,
    },
    MessageComplete {
        eof: &'step MessageEofSourceReceipt<'src>,
        outcome: &'step CompleteAdmissionOutcome<'src>,
        source: &'step DriverCompleteSource<'src>,
        effect: Option<&'step FrameAcceptEffect<'src>>,
        stats: &'step CanonicalProtobufAdmissionStats,
    },
    OtherContinue,
    OtherPush {
        pushed: &'step FramePushObservation,
        request: &'step FrameRequest<'src>,
    },
    OtherComplete {
        outcome: &'step CompleteAdmissionOutcome<'src>,
        source: &'step DriverCompleteSource<'src>,
        effect: Option<&'step FrameAcceptEffect<'src>>,
        stats: &'step CanonicalProtobufAdmissionStats,
    },
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckedAdmissionProjection {
    MessageContinue,
    MessagePush(FrameRequestKind),
    MessageComplete {
        scored: bool,
        root: bool,
        accepted_message_parent: bool,
    },
    OtherContinue(FrameRequestKind),
    OtherPush {
        owner: FrameRequestKind,
        child: FrameRequestKind,
    },
    OtherComplete {
        owner: FrameRequestKind,
        root: bool,
        accepted_message_parent: bool,
    },
}

#[derive(Clone, Copy)]
enum CheckedChildSummaries<'step, 'src> {
    MessageSlot {
        slot: usize,
        ordered: &'step [SemanticOccurrence<'src>],
    },
    MapValue,
    Reduced,
}

impl<'step, 'src> CheckedChildSummaries<'step, 'src> {
    fn from_completion(
        outcome: &CompleteAdmissionOutcome<'src>,
        after_frames: &'step [Frame<'src>],
    ) -> Option<Self> {
        match (outcome, after_frames.last()) {
            (CompleteAdmissionOutcome::AcceptedMessage(receipt), Some(Frame::Message(parent))) => {
                parent
                    .semantic_slots
                    .get(receipt.before.slot)
                    .map(|ordered| Self::MessageSlot {
                        slot: receipt.before.slot,
                        ordered: ordered.as_slice(),
                    })
            }
            (CompleteAdmissionOutcome::AcceptedOther, Some(Frame::Map(_))) => Some(Self::MapValue),
            (
                CompleteAdmissionOutcome::AcceptedOther,
                Some(Frame::EPathMap(_) | Frame::CanonicalPath(_)),
            ) => Some(Self::Reduced),
            _ => None,
        }
    }
}

struct CheckedAdmissionEvent<'step, 'src> {
    before: &'step DriverStepSource<'src>,
    after_frames: &'step [Frame<'src>],
    after_scores: &'step CanonicalScoreArena<'src>,
    action: CheckedAdmissionAction<'step, 'src>,
    map_source: Option<&'step MapStepSource<'src>>,
    accepted_children: Option<CheckedChildSummaries<'step, 'src>>,
}

impl CheckedAdmissionEvent<'_, '_> {
    fn map_push_parent_core_observation(
        &self,
    ) -> Option<MapPushCheckedParentCoreObservation<'_, '_>> {
        let CheckedAdmissionAction::MessageField {
            field,
            pushed: Some(pushed),
        } = &self.action
        else {
            return None;
        };
        let request = field.map_request.as_ref()?;
        let parent_index = pushed.before_len.checked_sub(1)?;
        let Frame::Message(parent) = self.after_frames.get(parent_index)? else {
            return None;
        };
        if pushed.request_kind != FrameRequestKind::Map
            || self.map_source.is_some()
            || self.before.top_kind != FrameRequestKind::Message
            || field.cursor_before != field.prestate.cursor
            || field.last_slot_before != field.prestate.last_slot
            || field.seen_before != field.prestate.seen
            || !self
                .before
                .matches_push_parent(self.after_frames, self.after_scores)
            || !field.matches_frame(parent, &FrameAction::Push(request.clone()))
        {
            return None;
        }
        let observed = MapPushCheckedParentCoreObservation::capture(parent, field)?;
        if !observed.body.same_view(&field.body)
            || observed.seen_before_at(field.resolved.slot()) != Some(field.seen_before)
        {
            return None;
        }
        Some(observed)
    }

    fn map_push_child_observation(&self) -> Option<MapPushCheckedChildObservation<'_, '_>> {
        let parent_core = self.map_push_parent_core_observation()?;
        let CheckedAdmissionAction::MessageField {
            field,
            pushed: Some(pushed),
        } = &self.action
        else {
            return None;
        };
        if !field.matches_pushed_driver(self.after_frames, pushed)
            || parent_core.semantic_slots.len() != field.slot_count
        {
            return None;
        }
        let Frame::Map(child) = self.after_frames.last()? else {
            return None;
        };
        Some(MapPushCheckedChildObservation::capture(child))
    }

    #[cfg(test)]
    fn projection(&self) -> CheckedAdmissionProjection {
        match &self.action {
            CheckedAdmissionAction::MessageField { pushed: None, .. } => {
                CheckedAdmissionProjection::MessageContinue
            }
            CheckedAdmissionAction::MessageField {
                pushed: Some(pushed),
                ..
            } => CheckedAdmissionProjection::MessagePush(pushed.request_kind),
            CheckedAdmissionAction::MessageComplete { eof, outcome, .. } => {
                CheckedAdmissionProjection::MessageComplete {
                    scored: eof.score_len_after > eof.score_len_before,
                    root: matches!(outcome, CompleteAdmissionOutcome::Root(_)),
                    accepted_message_parent: matches!(
                        outcome,
                        CompleteAdmissionOutcome::AcceptedMessage(_)
                    ),
                }
            }
            CheckedAdmissionAction::OtherContinue => {
                CheckedAdmissionProjection::OtherContinue(self.before.top_kind)
            }
            CheckedAdmissionAction::OtherPush { pushed, .. } => {
                CheckedAdmissionProjection::OtherPush {
                    owner: self.before.top_kind,
                    child: pushed.request_kind,
                }
            }
            CheckedAdmissionAction::OtherComplete { outcome, .. } => {
                CheckedAdmissionProjection::OtherComplete {
                    owner: self.before.top_kind,
                    root: matches!(outcome, CompleteAdmissionOutcome::Root(_)),
                    accepted_message_parent: matches!(
                        outcome,
                        CompleteAdmissionOutcome::AcceptedMessage(_)
                    ),
                }
            }
        }
    }
}

fn emit_checked_admission_event(
    event: CheckedAdmissionEvent<'_, '_>,
) -> Result<ObservedEvent, CanonicalProtobufAdmissionError> {
    let valid = match &event.action {
        CheckedAdmissionAction::MessageField {
            field,
            pushed: None,
        } => {
            event.map_source.is_none()
                && event.before.top_kind == FrameRequestKind::Message
                && event
                    .before
                    .matches_continue(event.after_frames, event.after_scores)
                && matches!(event.after_frames.last(), Some(Frame::Message(frame))
                    if field.matches_frame(frame, &FrameAction::Continue))
        }
        CheckedAdmissionAction::MessageField {
            field,
            pushed: Some(pushed),
        } => {
            if pushed.request_kind == FrameRequestKind::Map {
                event
                    .map_push_child_observation()
                    .is_some_and(|observed| observed.is_initial())
            } else {
                event.map_source.is_none()
                    && event.before.top_kind == FrameRequestKind::Message
                    && event
                        .before
                        .matches_push_parent(event.after_frames, event.after_scores)
                    && field.matches_pushed_driver(event.after_frames, pushed)
            }
        }
        CheckedAdmissionAction::MessageComplete {
            eof,
            outcome,
            source,
            effect,
            stats,
        } => {
            event.map_source.is_none()
                && event.before.top_kind == FrameRequestKind::Message
                && event
                    .before
                    .top_program
                    .is_some_and(|program| std::ptr::eq(program, eof.program))
                && source.stack_len_before == event.before.stack_len
                && source.completed_kind == event.before.top_kind
                && source
                    .completed_program
                    .is_some_and(|program| std::ptr::eq(program, eof.program))
                && source.score_len_before == event.before.score_entries
                && eof.score_len_before == event.before.score_entries
                && eof.score_len_after == event.after_scores.source_len()
                && event.before.stack_len.checked_sub(1) == Some(event.after_frames.len())
                && match outcome {
                    CompleteAdmissionOutcome::Root(summary) => {
                        effect.is_none()
                            && source.matches_root_return(
                                event.after_frames,
                                summary,
                                event.after_scores,
                                stats,
                            )
                    }
                    CompleteAdmissionOutcome::AcceptedMessage(_)
                    | CompleteAdmissionOutcome::AcceptedOther => {
                        effect.is_some()
                            && source.matches_accepted(
                                event.after_frames,
                                event.after_scores,
                                stats,
                                *effect,
                            )
                    }
                }
                && eof.matches_returned_summary(outcome, source, event.after_frames, *effect)
                && verify_complete_outcome(outcome, event.after_frames, event.after_scores).is_ok()
        }
        CheckedAdmissionAction::OtherContinue => {
            event.before.top_kind != FrameRequestKind::Message
                && event
                    .before
                    .matches_continue(event.after_frames, event.after_scores)
                && match (
                    event.before.top_kind,
                    event.map_source,
                    event.after_frames.last(),
                ) {
                    (
                        FrameRequestKind::Map,
                        Some(MapStepSource::Field(field)),
                        Some(Frame::Map(frame)),
                    ) => {
                        event.before.map_cursor == Some(field.cursor_before)
                            && field.matches_frame(frame, &FrameAction::Continue)
                    }
                    _ => false,
                }
        }
        CheckedAdmissionAction::OtherPush { pushed, request } => {
            let parent = pushed
                .before_len
                .checked_sub(1)
                .and_then(|index| event.after_frames.get(index));
            event.before.top_kind != FrameRequestKind::Message
                && event
                    .before
                    .matches_push_parent(event.after_frames, event.after_scores)
                && pushed.before_len == event.before.stack_len
                && pushed.after_len == event.after_frames.len()
                && pushed.request_kind == request.request_kind()
                && event
                    .after_frames
                    .last()
                    .is_some_and(|child| request.matches_pushed_frame(child))
                && match (event.before.top_kind, event.map_source, parent) {
                    (
                        FrameRequestKind::Map,
                        Some(MapStepSource::Field(field)),
                        Some(Frame::Map(_)),
                    ) => {
                        event.before.map_cursor == Some(field.cursor_before)
                            && field.matches_pushed_driver(event.after_frames, pushed)
                    }
                    (FrameRequestKind::EPathMap, None, Some(Frame::EPathMap(frame))) => {
                        frame.matches_push_request(request)
                    }
                    (FrameRequestKind::CanonicalPath, None, Some(Frame::CanonicalPath(frame))) => {
                        frame.matches_push_request(request)
                    }
                    _ => false,
                }
        }
        CheckedAdmissionAction::OtherComplete {
            outcome,
            source,
            effect,
            stats,
        } => {
            let source_matches = source.stack_len_before == event.before.stack_len
                && source.completed_kind == event.before.top_kind
                && source.score_len_before == event.before.score_entries
                && match outcome {
                    CompleteAdmissionOutcome::Root(summary) => {
                        effect.is_none()
                            && source.matches_root_return(
                                event.after_frames,
                                summary,
                                event.after_scores,
                                stats,
                            )
                    }
                    CompleteAdmissionOutcome::AcceptedMessage(_)
                    | CompleteAdmissionOutcome::AcceptedOther => {
                        effect.is_some()
                            && source.matches_accepted(
                                event.after_frames,
                                event.after_scores,
                                stats,
                                *effect,
                            )
                    }
                };
            event.before.top_kind != FrameRequestKind::Message
                && event.before.top_program.is_none()
                && event.before.stack_len.checked_sub(1) == Some(event.after_frames.len())
                && event.before.score_entries == event.after_scores.source_len()
                && verify_complete_outcome(outcome, event.after_frames, event.after_scores).is_ok()
                && source_matches
                && match (event.before.top_kind, event.map_source) {
                    (FrameRequestKind::Map, Some(MapStepSource::Eof(eof))) => {
                        event.before.map_cursor == Some(eof.cursor_after)
                            && event
                                .before
                                .map_eof_before
                                .as_ref()
                                .is_some_and(|before| before.matches_source(eof))
                            && match outcome {
                                CompleteAdmissionOutcome::Root(FrameSummary::Map(summary)) => {
                                    eof.summary.matches(summary)
                                }
                                CompleteAdmissionOutcome::Root(_) => false,
                                CompleteAdmissionOutcome::AcceptedMessage(receipt) => {
                                    matches!(&receipt.before.returned, FrameSummarySource::Map(accepted)
                                        if eof.summary.matches_source(accepted))
                                }
                                CompleteAdmissionOutcome::AcceptedOther => false,
                            }
                    }
                    (FrameRequestKind::EPathMap | FrameRequestKind::CanonicalPath, None) => {
                        event.before.map_cursor.is_none() && event.before.map_eof_before.is_none()
                    }
                    _ => false,
                }
        }
    };
    if !valid {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked admission event disagrees with its source transition",
        ));
    }
    if !accepted_children_agree(&event) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked admission event disagrees with its accepted child summaries",
        ));
    }
    let observed = observed_admission_event(&event)?;
    #[cfg(test)]
    rpb3_root_finish_trace::record_checked_admission_event(&event);
    Ok(observed)
}

fn emit_checked_admission_event_from_core<'src>(
    event: CheckedAdmissionEvent<'_, 'src>,
    ticket: &admission_live_core::PreStepTicket<'src>,
    epm_step_start: Option<&EPathMapLoopStateSource>,
    field_preimage: Option<&MessageFieldPreStepImage<'src>>,
) -> Result<ObservedEvent, CanonicalProtobufAdmissionError> {
    if !ticket.matches_checked_event(&event, epm_step_start) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked admission event disagrees with its live pre-step ticket",
        ));
    }
    let invalid_source_detail = match &event.action {
        CheckedAdmissionAction::MessageField {
            field,
            pushed: None,
        } => (!field_preimage.is_some_and(|before| {
            ticket.matches_field_preimage(before)
                && matches!(event.after_frames.last(), Some(Frame::Message(frame))
                    if before.matches_checked_scalar_receipt(field, frame))
        }))
        .then_some("checked message scalar iteration disagrees with its source image"),
        CheckedAdmissionAction::MessageField {
            field,
            pushed: Some(pushed),
        } => (!field_preimage.is_some_and(|before| {
            ticket.matches_field_preimage(before)
                && before.matches_checked_push_receipt(field, event.after_frames, pushed)
        }))
        .then_some("checked message push iteration disagrees with its source image"),
        _ => None,
    };
    let observed = emit_checked_admission_event(event)?;
    if let Some(detail) = invalid_source_detail {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(detail));
    }
    Ok(observed)
}

const MAP_ENTRY_SLOT_COUNT: usize = 2;

fn accepted_children_agree(event: &CheckedAdmissionEvent<'_, '_>) -> bool {
    let outcome = match &event.action {
        CheckedAdmissionAction::MessageComplete { outcome, .. }
        | CheckedAdmissionAction::OtherComplete { outcome, .. } => *outcome,
        CheckedAdmissionAction::MessageField { .. }
        | CheckedAdmissionAction::OtherContinue
        | CheckedAdmissionAction::OtherPush { .. } => return event.accepted_children.is_none(),
    };
    match (event.accepted_children, outcome, event.after_frames.last()) {
        (None, CompleteAdmissionOutcome::Root(_), None) => true,
        (
            Some(CheckedChildSummaries::MessageSlot { slot, ordered }),
            CompleteAdmissionOutcome::AcceptedMessage(receipt),
            Some(Frame::Message(parent)),
        ) => {
            slot == receipt.before.slot
                && ordered.len() == receipt.occurrences_after
                && parent
                    .semantic_slots
                    .get(slot)
                    .is_some_and(|stored| std::ptr::eq(stored.as_slice(), ordered))
                && ordered
                    .last()
                    .is_some_and(|occurrence| match &occurrence.body {
                        Some(body) => receipt.before.child_body.same_view(body),
                        None => receipt.before.child_kind == FrameRequestKind::Map,
                    })
        }
        (
            Some(CheckedChildSummaries::MapValue),
            CompleteAdmissionOutcome::AcceptedOther,
            Some(Frame::Map(_)),
        )
        | (
            Some(CheckedChildSummaries::Reduced),
            CompleteAdmissionOutcome::AcceptedOther,
            Some(Frame::EPathMap(_) | Frame::CanonicalPath(_)),
        ) => true,
        _ => false,
    }
}

fn observed_frame_kind(kind: FrameRequestKind) -> u8 {
    match kind {
        FrameRequestKind::Message => 1,
        FrameRequestKind::Map => 2,
        FrameRequestKind::EPathMap => 3,
        FrameRequestKind::CanonicalPath => 4,
    }
}

fn observed_frame_request(frame: &Frame<'_>) -> ObservedFrameRequest {
    let (body_len, slot_count) = match frame {
        Frame::Message(frame) => (frame.body.len(), frame.program.fields.len()),
        Frame::Map(frame) => (frame.region.len(), MAP_ENTRY_SLOT_COUNT),
        Frame::EPathMap(frame) => (frame.snapshot.len(), 0),
        Frame::CanonicalPath(frame) => (frame.input.len(), 0),
    };
    ObservedFrameRequest {
        kind: observed_frame_kind(frame.request_kind()),
        body_len,
        slot_count,
    }
}

fn observed_snapshot_after(
    frames: &[Frame<'_>],
    scores: &CanonicalScoreArena<'_>,
) -> ObservedSnapshot {
    ObservedSnapshot {
        depth: frames.len(),
        top_kind: frames
            .last()
            .map(|frame| observed_frame_kind(frame.request_kind())),
        score_len: scores.source_len(),
    }
}

fn observed_message_field(field: &MessageFieldSourceReceipt<'_>) -> ObservedField {
    ObservedField {
        slot: field.resolved.slot(),
        repeated: field.resolved.repeated(),
        cursor_after: field.cursor_after.bytes_read,
    }
}

fn observed_map_field(
    map_source: Option<&MapStepSource<'_>>,
) -> Result<Option<ObservedField>, CanonicalProtobufAdmissionError> {
    match map_source {
        None => Ok(None),
        Some(MapStepSource::Field(field)) => {
            let slot = usize::try_from(field.field_number)
                .ok()
                .and_then(|number| number.checked_sub(1))
                .ok_or(CanonicalProtobufAdmissionError::CutoverInvariant(
                    "checked map field number has no observable slot",
                ))?;
            Ok(Some(ObservedField {
                slot,
                repeated: false,
                cursor_after: field.cursor_after.bytes_read,
            }))
        }
        Some(MapStepSource::Eof(_)) => Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "checked map continuation carries an end-of-entry source",
        )),
    }
}

fn observed_pushed_child(
    after_frames: &[Frame<'_>],
) -> Result<ObservedFrameRequest, CanonicalProtobufAdmissionError> {
    after_frames.last().map(observed_frame_request).ok_or(
        CanonicalProtobufAdmissionError::CutoverInvariant("checked push left no child frame"),
    )
}

fn observed_accept_record(
    accepted_children: Option<CheckedChildSummaries<'_, '_>>,
) -> ObservedAcceptRecord {
    match accepted_children {
        None => ObservedAcceptRecord::Root,
        Some(CheckedChildSummaries::MessageSlot { slot, ordered }) => {
            ObservedAcceptRecord::MessageSlot {
                slot,
                occurrences_after: ordered.len(),
            }
        }
        Some(CheckedChildSummaries::MapValue) => ObservedAcceptRecord::MapValue,
        Some(CheckedChildSummaries::Reduced) => ObservedAcceptRecord::Reduced,
    }
}

fn observed_admission_event(
    event: &CheckedAdmissionEvent<'_, '_>,
) -> Result<ObservedEvent, CanonicalProtobufAdmissionError> {
    let before = ObservedSnapshot {
        depth: event.before.stack_len,
        top_kind: Some(observed_frame_kind(event.before.top_kind)),
        score_len: event.before.score_entries,
    };
    let after = observed_snapshot_after(event.after_frames, event.after_scores);
    Ok(match &event.action {
        CheckedAdmissionAction::MessageField {
            field,
            pushed: None,
        } => ObservedEvent::MessageContinue {
            before,
            field: observed_message_field(field),
            after,
        },
        CheckedAdmissionAction::MessageField {
            field,
            pushed: Some(_),
        } => ObservedEvent::MessagePush {
            before,
            field: observed_message_field(field),
            child: observed_pushed_child(event.after_frames)?,
            after,
        },
        CheckedAdmissionAction::MessageComplete { .. } => ObservedEvent::MessageComplete {
            before,
            record: observed_accept_record(event.accepted_children),
            after,
        },
        CheckedAdmissionAction::OtherContinue => ObservedEvent::OtherContinue {
            before,
            field: observed_map_field(event.map_source)?,
            after,
        },
        CheckedAdmissionAction::OtherPush { .. } => ObservedEvent::OtherPush {
            before,
            field: observed_map_field(event.map_source)?,
            child: observed_pushed_child(event.after_frames)?,
            after,
        },
        CheckedAdmissionAction::OtherComplete { .. } => ObservedEvent::OtherComplete {
            before,
            record: observed_accept_record(event.accepted_children),
            after,
        },
    })
}

#[derive(Clone, Copy)]
enum AdmissionStepKind {
    MessageContinue,
    MessagePush,
    MessageComplete,
    OtherContinue,
    OtherPush,
    OtherComplete,
}

// All large observations survive phase boundaries on the heap. The phases
// below borrow this single allocation; none calls the next phase while its
// own parser, completion, or return-source frame is live.
struct AdmissionScratch<'src> {
    request: Option<FrameRequest<'src>>,
    field: Option<MessageFieldSourceReceipt<'src>>,
    field_preimage: MessageFieldPreStepImage<'src>,
    field_input: Option<IndexedCanonicalField<'src>>,
    field_before: Option<MessageFieldBefore<'src>>,
    field_resolved: Option<ResolvedProtobufField>,
    field_action: Option<FrameAction<'src>>,
    epm_before: Option<EPathMapLoopStateSource>,
    epm_step_start: Option<EPathMapLoopStateSource>,
    #[cfg(test)]
    epm_terminal_before: Option<EPathMapLoopStateSource>,
    epm_event: Option<IndexedCanonicalEpm1TopologySourceEvent<'src>>,
    driver_source: Option<DriverStepSource<'src>>,
    summary: Option<FrameSummary<'src>>,
    eof: Option<MessageEofSourceReceipt<'src>>,
    map_source: Option<MapStepSource<'src>>,
    pushed: Option<FramePushObservation>,
    pushed_request: Option<FrameRequest<'src>>,
    complete_source: Option<DriverCompleteSource<'src>>,
    accept_effect: Option<FrameAcceptEffect<'src>>,
    message_accept_before: Option<MessageAcceptBefore<'src>>,
    outcome: Option<CompleteAdmissionOutcome<'src>>,
    #[cfg(test)]
    step_before: Option<Box<rpb3_root_finish_trace::DriverStepBefore>>,
}

impl<'src> AdmissionScratch<'src> {
    fn allocate() -> Box<Self> {
        // Writing each field into its final allocation avoids materializing a
        // large `Self` temporary in the already bounded caller stack frame.
        let mut allocation = Box::<Self>::new_uninit();
        let target = allocation.as_mut_ptr();
        // SAFETY: every field is initialized exactly once before assume_init;
        // no field is read while the allocation remains MaybeUninit.
        unsafe {
            std::ptr::addr_of_mut!((*target).request).write(None);
            std::ptr::addr_of_mut!((*target).field).write(None);
            std::ptr::addr_of_mut!((*target).field_preimage).write(MessageFieldPreStepImage::new());
            std::ptr::addr_of_mut!((*target).field_input).write(None);
            std::ptr::addr_of_mut!((*target).field_before).write(None);
            std::ptr::addr_of_mut!((*target).field_resolved).write(None);
            std::ptr::addr_of_mut!((*target).field_action).write(None);
            std::ptr::addr_of_mut!((*target).epm_before).write(None);
            std::ptr::addr_of_mut!((*target).epm_step_start).write(None);
            #[cfg(test)]
            std::ptr::addr_of_mut!((*target).epm_terminal_before).write(None);
            std::ptr::addr_of_mut!((*target).epm_event).write(None);
            std::ptr::addr_of_mut!((*target).driver_source).write(None);
            std::ptr::addr_of_mut!((*target).summary).write(None);
            std::ptr::addr_of_mut!((*target).eof).write(None);
            std::ptr::addr_of_mut!((*target).map_source).write(None);
            std::ptr::addr_of_mut!((*target).pushed).write(None);
            std::ptr::addr_of_mut!((*target).pushed_request).write(None);
            std::ptr::addr_of_mut!((*target).complete_source).write(None);
            std::ptr::addr_of_mut!((*target).accept_effect).write(None);
            std::ptr::addr_of_mut!((*target).message_accept_before).write(None);
            std::ptr::addr_of_mut!((*target).outcome).write(None);
            #[cfg(test)]
            std::ptr::addr_of_mut!((*target).step_before).write(None);
            allocation.assume_init()
        }
    }

    fn clear(&mut self) {
        self.request = None;
        self.field = None;
        self.field_preimage.clear();
        self.field_input = None;
        self.field_before = None;
        self.field_resolved = None;
        self.field_action = None;
        self.epm_before = None;
        self.epm_step_start = None;
        #[cfg(test)]
        {
            self.epm_terminal_before = None;
        }
        self.epm_event = None;
        self.driver_source = None;
        self.summary = None;
        self.eof = None;
        self.map_source = None;
        self.pushed = None;
        self.pushed_request = None;
        self.complete_source = None;
        self.accept_effect = None;
        self.message_accept_before = None;
        self.outcome = None;
        #[cfg(test)]
        {
            self.step_before = None;
        }
    }

    fn driver_source(&self) -> &DriverStepSource<'src> {
        self.driver_source
            .as_ref()
            .expect("admission step has a source observation")
    }

    fn store_other(
        &mut self,
        action: FrameAction<'src>,
        map_source: Option<MapStepSource<'src>>,
    ) -> AdmissionStepKind {
        self.map_source = map_source;
        match action {
            FrameAction::Continue => AdmissionStepKind::OtherContinue,
            FrameAction::Push(request) => {
                self.request = Some(request);
                AdmissionStepKind::OtherPush
            }
            FrameAction::Complete(summary) => {
                self.summary = Some(summary);
                AdmissionStepKind::OtherComplete
            }
        }
    }

    #[cfg(test)]
    fn take_test_step(&mut self, kind: AdmissionStepKind) -> FrameStep<'src> {
        match kind {
            AdmissionStepKind::MessageContinue => {
                FrameStep::MessageContinue(self.field.take().expect("checked message field"))
            }
            AdmissionStepKind::MessagePush => FrameStep::MessagePush {
                request: self.request.take().expect("message child request"),
                field: self.field.take().expect("checked message field"),
            },
            AdmissionStepKind::MessageComplete => FrameStep::MessageComplete {
                summary: self.summary.take().expect("message completion summary"),
                eof: self.eof.take().expect("checked message EOF receipt"),
            },
            AdmissionStepKind::OtherContinue => FrameStep::Other {
                action: FrameAction::Continue,
                map_source: self.map_source.take(),
            },
            AdmissionStepKind::OtherPush => FrameStep::Other {
                action: FrameAction::Push(self.request.take().expect("nonmessage child request")),
                map_source: self.map_source.take(),
            },
            AdmissionStepKind::OtherComplete => FrameStep::Other {
                action: FrameAction::Complete(
                    self.summary.take().expect("nonmessage completion summary"),
                ),
                map_source: self.map_source.take(),
            },
        }
    }
}

#[inline(never)]
fn prepare_admission_completion<'src>(
    driver_source: &DriverStepSource<'src>,
    summary: &FrameSummary<'src>,
    eof: Option<&MessageEofSourceReceipt<'src>>,
    complete_source: &mut Option<DriverCompleteSource<'src>>,
    message_accept_before: &mut Option<MessageAcceptBefore<'src>>,
    frames: &[Frame<'src>],
    scores: &CanonicalScoreArena<'src>,
    stats: &CanonicalProtobufAdmissionStats,
) -> Result<(), CanonicalProtobufAdmissionError> {
    if (driver_source.top_kind == FrameRequestKind::Message) != eof.is_some()
        || eof.is_some_and(|eof| {
            eof.score_len_before != driver_source.score_entries
                || !matches!(frames.last(), Some(Frame::Message(frame))
                    if eof.matches_frame(frame, summary, scores))
        })
    {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "message completion omitted its checked EOF source receipt",
        ));
    }
    *complete_source = Some(
        DriverCompleteSource::before_pop(driver_source, frames, summary, scores, stats).ok_or(
            CanonicalProtobufAdmissionError::CutoverInvariant(
                "admission completion disagrees with its source frame and summary",
            ),
        )?,
    );
    *message_accept_before = match frames
        .len()
        .checked_sub(2)
        .and_then(|index| frames.get(index))
    {
        Some(Frame::Message(parent)) => Some(
            MessageAcceptBefore::capture(
                parent,
                frames.last().expect("completed child remains before pop"),
                summary,
                frames.len(),
                driver_source.score_entries,
                scores.source_len(),
            )
            .ok_or(CanonicalProtobufAdmissionError::CutoverInvariant(
                "message parent acceptance lacks a checked return source",
            ))?,
        ),
        _ => None,
    };
    Ok(())
}

#[inline(never)]
fn pop_admission_completion<'src>(
    scratch: &AdmissionScratch<'src>,
    frames: &mut Vec<Frame<'src>>,
    scores: &CanonicalScoreArena<'src>,
    stats: &CanonicalProtobufAdmissionStats,
) -> Result<(), CanonicalProtobufAdmissionError> {
    #[cfg(test)]
    rpb3_root_finish_trace::record_pre_pop(
        frames,
        scratch
            .summary
            .as_ref()
            .expect("completion owns its summary"),
        scores,
    );
    frames.truncate(frames.len() - 1);
    if !scratch
        .complete_source
        .as_ref()
        .expect("prepared completion source")
        .matches_popped(frames, scores, stats)
    {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "admission pop changed its suspended parent or score arena",
        ));
    }
    Ok(())
}

#[inline(never)]
fn accept_admission_completion<'src>(
    scratch: &mut AdmissionScratch<'src>,
    frames: &mut Vec<Frame<'src>>,
    scores: &CanonicalScoreArena<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let summary = scratch.summary.take().expect("completion owns its summary");
    let complete_source = scratch
        .complete_source
        .as_ref()
        .expect("prepared completion source");
    #[cfg(test)]
    let after_len = frames.len();
    if let Some(parent) = frames.last_mut() {
        let effect = parent.accept(summary, stats, scores)?;
        if !complete_source.matches_accepted(frames, scores, stats, Some(&effect)) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "admission parent acceptance disagrees with its source continuation",
            ));
        }
        #[cfg(test)]
        rpb3_root_finish_trace::record_parent_accept(
            frames.last().expect("accepted parent remains on the stack"),
            &effect,
            scores,
            after_len,
        );
        #[cfg(test)]
        rpb3_root_finish_trace::record_driver_step(
            scratch.step_before.take(),
            rpb3_root_finish_trace::DriverStepKind::Complete,
            frames,
            scores,
        );
        scratch.outcome = Some(match scratch.message_accept_before.take() {
            Some(before) => {
                let Some(Frame::Message(parent)) = frames.last() else {
                    return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                        "accepted message parent changed frame kind",
                    ));
                };
                CompleteAdmissionOutcome::AcceptedMessage(
                    before.after(parent, frames.len(), scores).ok_or(
                        CanonicalProtobufAdmissionError::CutoverInvariant(
                            "accepted message parent disagrees with its checked return receipt",
                        ),
                    )?,
                )
            }
            None => CompleteAdmissionOutcome::AcceptedOther,
        });
        scratch.accept_effect = Some(effect);
    } else {
        if !complete_source.matches_root_return(frames, &summary, scores, stats) {
            return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                "admission root return changed frame storage or score arena",
            ));
        }
        #[cfg(test)]
        rpb3_root_finish_trace::record_return(&summary, scores, frames.len());
        #[cfg(test)]
        rpb3_root_finish_trace::record_driver_step(
            scratch.step_before.take(),
            rpb3_root_finish_trace::DriverStepKind::Complete,
            frames,
            scores,
        );
        scratch.outcome = Some(CompleteAdmissionOutcome::Root(summary));
    }
    Ok(())
}

// Existing property tests inspect a completion directly. This adapter runs
// the same sibling phases but is not called by the production driver.
#[cfg(test)]
#[expect(
    dead_code,
    reason = "no property test calls this completion adapter; every caller uses complete_admission_step_with_source, which also returns the completion source and accept effect"
)]
fn complete_admission_step<'src>(
    driver_source: &DriverStepSource<'src>,
    frames: &mut Vec<Frame<'src>>,
    summary: FrameSummary<'src>,
    message_eof: Option<&MessageEofSourceReceipt<'src>>,
    scores: &CanonicalScoreArena<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    step_before: Option<Box<rpb3_root_finish_trace::DriverStepBefore>>,
) -> Result<CompleteAdmissionOutcome<'src>, CanonicalProtobufAdmissionError> {
    complete_admission_step_with_source(
        driver_source,
        frames,
        summary,
        message_eof,
        scores,
        stats,
        step_before,
    )
    .map(|(outcome, _, _)| outcome)
}

#[cfg(test)]
fn complete_admission_step_with_source<'src>(
    driver_source: &DriverStepSource<'src>,
    frames: &mut Vec<Frame<'src>>,
    summary: FrameSummary<'src>,
    message_eof: Option<&MessageEofSourceReceipt<'src>>,
    scores: &CanonicalScoreArena<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
    step_before: Option<Box<rpb3_root_finish_trace::DriverStepBefore>>,
) -> Result<
    (
        CompleteAdmissionOutcome<'src>,
        DriverCompleteSource<'src>,
        Option<FrameAcceptEffect<'src>>,
    ),
    CanonicalProtobufAdmissionError,
> {
    let mut scratch = AdmissionScratch::allocate();
    scratch.summary = Some(summary);
    scratch.step_before = step_before;
    prepare_admission_completion(
        driver_source,
        scratch.summary.as_ref().expect("test completion summary"),
        message_eof,
        &mut scratch.complete_source,
        &mut scratch.message_accept_before,
        frames,
        scores,
        stats,
    )?;
    pop_admission_completion(&scratch, frames, scores, stats)?;
    accept_admission_completion(&mut scratch, frames, scores, stats)?;
    Ok((
        scratch.outcome.take().expect("test completion outcome"),
        scratch
            .complete_source
            .take()
            .expect("test completion source"),
        scratch.accept_effect.take(),
    ))
}

fn verify_complete_outcome<'src>(
    outcome: &CompleteAdmissionOutcome<'src>,
    frames: &[Frame<'src>],
    scores: &CanonicalScoreArena<'src>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    match outcome {
        CompleteAdmissionOutcome::AcceptedMessage(receipt) => {
            if !matches!(frames.last(), Some(Frame::Message(frame))
                if receipt.matches_frame(frame)
                    && receipt.score_len_after_accept == scores.source_len())
            {
                return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                    "message acceptance omitted its checked driver return receipt",
                ));
            }
            Ok(())
        }
        CompleteAdmissionOutcome::AcceptedOther => {
            if frames.is_empty() || matches!(frames.last(), Some(Frame::Message(_))) {
                return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                    "nonmessage acceptance changed the parent frame kind",
                ));
            }
            Ok(())
        }
        CompleteAdmissionOutcome::Root(_) => {
            if !frames.is_empty() {
                return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                    "admission root returned with live frames",
                ));
            }
            Ok(())
        }
    }
}

#[inline(never)]
fn process_admission_noncompletion<'src>(
    kind: AdmissionStepKind,
    scratch: &mut AdmissionScratch<'src>,
    frames: &mut Vec<Frame<'src>>,
    scores: &CanonicalScoreArena<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<(), CanonicalProtobufAdmissionError> {
    match kind {
        AdmissionStepKind::MessageContinue => check_message_continue(scratch, frames, scores),
        AdmissionStepKind::MessagePush => {
            let request_kind = check_message_push_before(scratch, frames)?;
            push_admission_frame_into(
                frames,
                &mut scratch.request,
                stats,
                &mut scratch.pushed,
                None,
            )?;
            check_message_push_after(scratch, frames, scores, stats, request_kind)
        }
        AdmissionStepKind::OtherContinue => check_other_continue(scratch, frames, scores),
        AdmissionStepKind::OtherPush => {
            let request_kind = check_other_push_before(scratch, frames)?;
            push_admission_frame_into(
                frames,
                &mut scratch.request,
                stats,
                &mut scratch.pushed,
                Some(&mut scratch.pushed_request),
            )?;
            check_other_push_after(scratch, frames, scores, stats, request_kind)
        }
        _ => unreachable!("admission step tag and stored action agree"),
    }
}

#[inline(never)]
fn check_message_continue<'src>(
    scratch: &mut AdmissionScratch<'src>,
    frames: &Vec<Frame<'src>>,
    scores: &CanonicalScoreArena<'src>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let driver_source = scratch
        .driver_source
        .as_ref()
        .expect("admission step source");
    let field = scratch.field.as_ref().expect("checked message field");
    let Some(Frame::Message(frame)) = frames.last() else {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "message continue lost its source frame",
        ));
    };
    if driver_source.top_kind != FrameRequestKind::Message
        || !field.matches_frame(frame, &FrameAction::Continue)
        || !driver_source.matches_continue(frames, scores)
    {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "message continue disagrees with its checked field source",
        ));
    }
    #[cfg(test)]
    rpb3_root_finish_trace::record_message_field(frame, &field, &field.field);
    #[cfg(test)]
    rpb3_root_finish_trace::record_driver_step(
        scratch.step_before.take(),
        rpb3_root_finish_trace::DriverStepKind::Continue,
        frames,
        scores,
    );
    Ok(())
}

#[inline(never)]
fn check_message_push_before<'src>(
    scratch: &AdmissionScratch<'src>,
    frames: &[Frame<'src>],
) -> Result<FrameRequestKind, CanonicalProtobufAdmissionError> {
    let driver_source = scratch
        .driver_source
        .as_ref()
        .expect("admission step source");
    let field = scratch.field.as_ref().expect("checked message field");
    let request = scratch.request.as_ref().expect("message child request");
    if driver_source.top_kind != FrameRequestKind::Message
        || !matches!(frames.last(), Some(Frame::Message(parent))
            if field.matches_frame(parent, &FrameAction::Push((*request).clone())))
    {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "message push lost its checked field request",
        ));
    }
    Ok(request.request_kind())
}

#[inline(never)]
fn check_message_push_after<'src>(
    scratch: &mut AdmissionScratch<'src>,
    frames: &Vec<Frame<'src>>,
    scores: &CanonicalScoreArena<'src>,
    stats: &CanonicalProtobufAdmissionStats,
    request_kind: FrameRequestKind,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let driver_source = scratch
        .driver_source
        .as_ref()
        .expect("admission step source");
    let field = scratch.field.as_ref().expect("checked message field");
    let pushed = *scratch.pushed.as_ref().expect("checked message push");
    if !driver_source.matches_push_parent(frames, scores)
        || pushed.request_kind != request_kind
        || pushed.frame_variant_bytes != stats.frame_variant_bytes
        || pushed.after_len != frames.len()
        || pushed.after_capacity != frames.capacity()
        || !field.matches_pushed_driver(frames, &pushed)
    {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "message push disagrees with its checked child and storage source",
        ));
    }
    #[cfg(test)]
    {
        let Frame::Message(parent) = &frames[pushed.before_len - 1] else {
            unreachable!("checked message push retains its parent frame")
        };
        rpb3_root_finish_trace::record_message_field(parent, &field, &field.field);
    }
    #[cfg(test)]
    rpb3_root_finish_trace::record_driver_step(
        scratch.step_before.take(),
        rpb3_root_finish_trace::DriverStepKind::Push(pushed),
        frames,
        scores,
    );
    Ok(())
}

#[inline(never)]
fn check_other_continue<'src>(
    scratch: &mut AdmissionScratch<'src>,
    frames: &Vec<Frame<'src>>,
    scores: &CanonicalScoreArena<'src>,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let driver_source = scratch
        .driver_source
        .as_ref()
        .expect("admission step source");
    if driver_source.top_kind == FrameRequestKind::Message {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "message frame omitted its typed source step",
        ));
    }
    if !driver_source.matches_continue(frames, scores) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "admission continue changed frame storage or the score arena",
        ));
    }
    #[cfg(test)]
    rpb3_root_finish_trace::record_driver_step(
        scratch.step_before.take(),
        rpb3_root_finish_trace::DriverStepKind::Continue,
        frames,
        scores,
    );
    Ok(())
}

#[inline(never)]
fn check_other_push_before<'src>(
    scratch: &AdmissionScratch<'src>,
    frames: &[Frame<'src>],
) -> Result<FrameRequestKind, CanonicalProtobufAdmissionError> {
    let driver_source = scratch
        .driver_source
        .as_ref()
        .expect("admission step source");
    let request = scratch.request.as_ref().expect("nonmessage child request");
    if driver_source.top_kind == FrameRequestKind::Message {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "message frame omitted its typed source step",
        ));
    }
    if frames.last().is_some_and(|parent| match parent {
        Frame::EPathMap(epm) => !epm.matches_push_request(request),
        Frame::CanonicalPath(path) => !path.matches_push_request(request),
        _ => false,
    }) {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "path push request disagrees with its retained child and obligation",
        ));
    }
    #[cfg(test)]
    rpb3_root_finish_trace::record_nonmessage_push(frames, request);
    Ok(request.request_kind())
}

#[inline(never)]
fn check_other_push_after<'src>(
    scratch: &mut AdmissionScratch<'src>,
    frames: &Vec<Frame<'src>>,
    scores: &CanonicalScoreArena<'src>,
    stats: &CanonicalProtobufAdmissionStats,
    request_kind: FrameRequestKind,
) -> Result<(), CanonicalProtobufAdmissionError> {
    let driver_source = scratch
        .driver_source
        .as_ref()
        .expect("admission step source");
    let pushed = *scratch.pushed.as_ref().expect("checked nonmessage push");
    if !driver_source.matches_push_parent(frames, scores)
        || pushed.request_kind != request_kind
        || pushed.frame_variant_bytes != stats.frame_variant_bytes
        || pushed.after_len != frames.len()
        || pushed.after_capacity != frames.capacity()
    {
        return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
            "admission push disagrees with its checked storage observation",
        ));
    }
    #[cfg(test)]
    rpb3_root_finish_trace::record_driver_step(
        scratch.step_before.take(),
        rpb3_root_finish_trace::DriverStepKind::Push(pushed),
        frames,
        scores,
    );
    Ok(())
}

#[inline(never)]
fn emit_admission_noncompletion_event<'src>(
    kind: AdmissionStepKind,
    scratch: &AdmissionScratch<'src>,
    frames: &[Frame<'src>],
    scores: &CanonicalScoreArena<'src>,
    ticket: &admission_live_core::PreStepTicket<'src>,
) -> Result<ObservedEvent, CanonicalProtobufAdmissionError> {
    let action = match kind {
        AdmissionStepKind::MessageContinue | AdmissionStepKind::MessagePush => {
            CheckedAdmissionAction::MessageField {
                field: scratch.field.as_ref().expect("checked message field"),
                pushed: scratch.pushed.as_ref(),
            }
        }
        AdmissionStepKind::OtherContinue => CheckedAdmissionAction::OtherContinue,
        AdmissionStepKind::OtherPush => CheckedAdmissionAction::OtherPush {
            pushed: scratch.pushed.as_ref().expect("checked nonmessage push"),
            request: scratch
                .pushed_request
                .as_ref()
                .expect("checked nonmessage push source"),
        },
        _ => unreachable!("noncompletion event kind"),
    };
    emit_checked_admission_event_from_core(
        CheckedAdmissionEvent {
            before: scratch.driver_source(),
            after_frames: frames,
            after_scores: scores,
            action,
            map_source: scratch.map_source.as_ref(),
            accepted_children: None,
        },
        ticket,
        scratch.epm_step_start.as_ref(),
        Some(&scratch.field_preimage),
    )
}

#[inline(never)]
fn process_admission_completion<'src>(
    kind: AdmissionStepKind,
    scratch: &mut AdmissionScratch<'src>,
    frames: &mut Vec<Frame<'src>>,
    scores: &CanonicalScoreArena<'src>,
    stats: &mut CanonicalProtobufAdmissionStats,
) -> Result<bool, CanonicalProtobufAdmissionError> {
    // The returned summary and receipts already occupy heap scratch slots.
    match kind {
        AdmissionStepKind::MessageComplete => {
            debug_assert!(scratch.summary.is_some() && scratch.eof.is_some());
        }
        AdmissionStepKind::OtherComplete => {
            if scratch.driver_source().top_kind == FrameRequestKind::Message {
                return Err(CanonicalProtobufAdmissionError::CutoverInvariant(
                    "message frame omitted its typed source step",
                ));
            }
            debug_assert!(scratch.summary.is_some());
        }
        _ => unreachable!("admission completion tag and stored action agree"),
    }
    prepare_admission_completion(
        scratch
            .driver_source
            .as_ref()
            .expect("admission step source"),
        scratch
            .summary
            .as_ref()
            .expect("completion owns its summary"),
        scratch.eof.as_ref(),
        &mut scratch.complete_source,
        &mut scratch.message_accept_before,
        frames,
        scores,
        stats,
    )?;
    pop_admission_completion(scratch, frames, scores, stats)?;
    accept_admission_completion(scratch, frames, scores, stats)?;
    let outcome = scratch
        .outcome
        .as_ref()
        .expect("accepted completion outcome");
    verify_complete_outcome(outcome, frames, scores)?;
    Ok(matches!(outcome, CompleteAdmissionOutcome::Root(_)))
}

#[inline(never)]
fn emit_admission_completion_event<'src>(
    kind: AdmissionStepKind,
    scratch: &AdmissionScratch<'src>,
    frames: &[Frame<'src>],
    scores: &CanonicalScoreArena<'src>,
    stats: &CanonicalProtobufAdmissionStats,
    ticket: &admission_live_core::PreStepTicket<'src>,
) -> Result<ObservedEvent, CanonicalProtobufAdmissionError> {
    let outcome = scratch
        .outcome
        .as_ref()
        .expect("accepted completion outcome");
    let accepted_children = CheckedChildSummaries::from_completion(outcome, frames);
    #[cfg(test)]
    let accepted_children = verified_monitor_seam::seamed_children(accepted_children);
    emit_checked_admission_event_from_core(
        CheckedAdmissionEvent {
            before: scratch.driver_source(),
            after_frames: frames,
            after_scores: scores,
            action: match kind {
                AdmissionStepKind::MessageComplete => CheckedAdmissionAction::MessageComplete {
                    eof: scratch.eof.as_ref().expect("checked message EOF receipt"),
                    outcome,
                    source: scratch
                        .complete_source
                        .as_ref()
                        .expect("checked completion source"),
                    effect: scratch.accept_effect.as_ref(),
                    stats,
                },
                AdmissionStepKind::OtherComplete => CheckedAdmissionAction::OtherComplete {
                    outcome,
                    source: scratch
                        .complete_source
                        .as_ref()
                        .expect("checked completion source"),
                    effect: scratch.accept_effect.as_ref(),
                    stats,
                },
                _ => unreachable!("completion tag"),
            },
            map_source: scratch.map_source.as_ref(),
            accepted_children,
        },
        ticket,
        scratch.epm_step_start.as_ref(),
        None,
    )
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
    admission_live_core::AdmissionLiveCore::new(initial, input_bytes)?.run()
}

// The production driver and its private pre-step ticket live together. No
// caller can manufacture a checked action independently of a core-owned step.
#[path = "admission_live_core.rs"]
mod admission_live_core;

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "d693151fc added this private standalone canonical-key root without activating the importer, which remains open under task 6018"
    )
)]
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
    let canonical_root = wire_program_by_type_id(root.type_id).map_err(|error| {
        CanonicalProtobufAdmissionError::Schema {
            type_name: root.type_name,
            field: 0,
            position: root_start,
            detail: error.detail(),
        }
    })?;
    if !std::ptr::eq(root, canonical_root) {
        return Err(CanonicalProtobufAdmissionError::Schema {
            type_name: root.type_name,
            field: 0,
            position: root_start,
            detail: "protobuf root is not the canonical wire program for its type ID",
        });
    }
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
#[path = "../../../tests/support/rpb3_root_finish_trace.rs"]
mod rpb3_root_finish_trace;

#[cfg(test)]
#[path = "rpb3_root_projection_properties.rs"]
mod rpb3_root_projection_properties;

#[cfg(test)]
#[path = "rpb3_generated_oneof_seen_properties.rs"]
mod rpb3_generated_oneof_seen_properties;

#[cfg(test)]
#[path = "rpb3_message_eof_trace_properties.rs"]
mod rpb3_message_eof_trace_properties;

#[cfg(test)]
#[path = "rpb3_message_complete_checked_event_properties.rs"]
mod rpb3_message_complete_checked_event_properties;

#[cfg(test)]
#[path = "rpb3_generic_message_field_source_properties.rs"]
mod rpb3_generic_message_field_source_properties;

#[cfg(test)]
#[path = "rpb3_message_scalar_iteration_image_properties.rs"]
mod rpb3_message_scalar_iteration_image_properties;

#[cfg(test)]
#[path = "rpb3_canonical_path_root_complete_properties.rs"]
mod rpb3_canonical_path_root_complete_properties;

#[cfg(test)]
#[path = "rpb3_canonical_path_root_escape_properties.rs"]
mod rpb3_canonical_path_root_escape_properties;

#[cfg(test)]
#[path = "rpb3_epathmap_batch_scalar_properties.rs"]
mod rpb3_epathmap_batch_scalar_properties;

#[cfg(test)]
#[path = "rpb3_epathmap_multikey_batch_properties.rs"]
mod rpb3_epathmap_multikey_batch_properties;

#[cfg(test)]
#[path = "rpb3_canonical_path_root_nested_epm_properties.rs"]
mod rpb3_canonical_path_root_nested_epm_properties;

#[cfg(test)]
#[path = "rpb3_canonical_path_return_source_properties.rs"]
mod rpb3_canonical_path_return_source_properties;

#[cfg(test)]
#[path = "rpb3_nested_epathmap_return_view_properties.rs"]
mod rpb3_nested_epathmap_return_view_properties;

#[cfg(test)]
#[path = "rpb3_canonical_path_terminal_source_properties.rs"]
mod rpb3_canonical_path_terminal_source_properties;

#[cfg(test)]
#[path = "rpb3_canonical_path_root_push_source_properties.rs"]
mod rpb3_canonical_path_root_push_source_properties;

#[cfg(test)]
#[path = "rpb3_map_key_source_properties.rs"]
mod rpb3_map_key_source_properties;

#[cfg(test)]
#[path = "rpb3_map_checked_event_properties.rs"]
mod rpb3_map_checked_event_properties;

#[cfg(test)]
#[path = "rpb3_map_push_receipt_source_properties.rs"]
mod rpb3_map_push_receipt_source_properties;
#[cfg(test)]
#[path = "rpb3_map_push_fresh_child_properties.rs"]
mod rpb3_map_push_fresh_child_properties;
#[cfg(test)]
#[path = "rpb3_map_push_live_child_projection_properties.rs"]
mod rpb3_map_push_live_child_projection_properties;

#[cfg(test)]
#[path = "rpb3_raw_region_tree_properties.rs"]
mod rpb3_raw_region_tree_properties;

#[cfg(test)]
#[path = "rpb3_par_source_summary_properties.rs"]
mod rpb3_par_source_summary_properties;

#[cfg(test)]
#[path = "rpb3_scalar_source_properties.rs"]
mod rpb3_scalar_source_properties;

#[cfg(test)]
#[path = "rpb3_source_reference_integration_properties.rs"]
mod rpb3_source_reference_integration_properties;

#[cfg(test)]
#[path = "rpb3_epathmap_terminal_source_properties.rs"]
mod rpb3_epathmap_terminal_source_properties;

#[cfg(test)]
#[path = "rpb3_score_sequence_properties.rs"]
mod rpb3_score_sequence_properties;

#[cfg(test)]
#[path = "rpb3_epathmap_receipt_sequence_properties.rs"]
mod rpb3_epathmap_receipt_sequence_properties;

#[cfg(test)]
#[path = "rpb3_active_frame_sequence_properties.rs"]
mod rpb3_active_frame_sequence_properties;

#[cfg(test)]
#[path = "rpb3_nested_epathmap_active_frame_properties.rs"]
mod rpb3_nested_epathmap_active_frame_properties;

#[cfg(test)]
#[path = "rpb3_epathmap_obligation_return_properties.rs"]
mod rpb3_epathmap_obligation_return_properties;

#[cfg(test)]
#[path = "rpb3_staged_admission_properties.rs"]
mod rpb3_staged_admission_properties;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use proptest::prelude::*;
    use prost::Message;

    use super::*;
    use crate::rhoapi::connective::ConnectiveInstance;
    use crate::rhoapi::cost_signature_atom::Form;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::g_unforgeable::UnfInstance;
    use crate::rhoapi::tagged_continuation::TaggedCont;
    use crate::rhoapi::var::VarInstance;
    use crate::rhoapi::{
        BindPattern, Bundle, Connective, ConnectiveBody, CostBoundSignature, CostClauseRegion,
        CostRegion, CostSignature, CostSignatureAtom, CostTokenStack, EMap, ESet, EVar, Expr,
        GBigRational, GDeployId, GDeployerId, GFixedPoint, GPrivate, GSysAuthToken, GUnforgeable,
        If, KeyValuePair, ListBindPatterns, Match, MatchCase, New, Par, ParWithRandom, Receive,
        ReceiveBind, Send, TaggedContinuation, Var, VarRef,
    };
    use crate::rust::par_bag::{with_component_key, ParBagComponent, PAR_BAG_KEY_DOMAIN};
    use crate::rust::rhoapi_ext::EPathMap;
    use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
    use crate::rust::rholang::canonical_score_bytes::{
        CanonicalScoreSourceEntry, CanonicalScoreSourceKind,
    };
    use crate::rust::rholang::match_schema::{RhoMatchFieldKind, RhoMatchPayloadKind};
    use crate::rust::rholang::match_schema_tables::{
        BINDPATTERN_RHO_MATCH_PROGRAM, BUNDLE_RHO_MATCH_PROGRAM, CONNECTIVEBODY_RHO_MATCH_PROGRAM,
        CONNECTIVE_RHO_MATCH_PROGRAM, COSTBOUNDSIGNATURE_RHO_MATCH_PROGRAM,
        COSTCLAUSEREGION_RHO_MATCH_PROGRAM, COSTREGION_RHO_MATCH_PROGRAM,
        COSTSIGNATUREATOM_RHO_MATCH_PROGRAM, COSTSIGNATURE_RHO_MATCH_PROGRAM,
        EMAP_RHO_MATCH_PROGRAM, ENEG_RHO_MATCH_PROGRAM, EPLUS_RHO_MATCH_PROGRAM,
        EVAR_RHO_MATCH_PROGRAM, EXPR_RHO_MATCH_PROGRAM, GBIGRATIONAL_RHO_MATCH_PROGRAM,
        GDEPLOYERID_RHO_MATCH_PROGRAM, GDEPLOYID_RHO_MATCH_PROGRAM, GFIXEDPOINT_RHO_MATCH_PROGRAM,
        GPRIVATE_RHO_MATCH_PROGRAM, GSYSAUTHTOKEN_RHO_MATCH_PROGRAM,
        GUNFORGEABLE_RHO_MATCH_PROGRAM, IF_RHO_MATCH_PROGRAM, KEYVALUEPAIR_RHO_MATCH_PROGRAM,
        LISTBINDPATTERNS_RHO_MATCH_PROGRAM, MATCHCASE_RHO_MATCH_PROGRAM, MATCH_RHO_MATCH_PROGRAM,
        NEW_RHO_MATCH_PROGRAM, PARWITHRANDOM_RHO_MATCH_PROGRAM, PAR_RHO_MATCH_PROGRAM,
        RECEIVEBIND_RHO_MATCH_PROGRAM, RECEIVE_RHO_MATCH_PROGRAM, RHO_MATCH_REGISTRY,
        SEND_RHO_MATCH_PROGRAM, TAGGEDCONTINUATION_RHO_MATCH_PROGRAM, VARREF_RHO_MATCH_PROGRAM,
        VAR_RHO_MATCH_PROGRAM, VAR_WILDCARDMSG_RHO_MATCH_PROGRAM,
    };
    use crate::rust::rholang::protobuf_schema::{ProtobufField, ProtobufNode};
    use crate::rust::rholang::schema_test_adapters::{
        explicit_default_mutants, schema_fixtures, GENERATED_SCHEMA_TEST_EXTERN_COUNT,
        GENERATED_SCHEMA_TEST_EXTERN_NAMES, GENERATED_SCHEMA_TEST_MESSAGE_COUNT,
        GENERATED_SCHEMA_TEST_ONEOF_ARM_COUNT, GENERATED_SCHEMA_TEST_ONEOF_COUNT,
    };
    use crate::rust::rholang::{bincode_encoder, protobuf_decoder, protobuf_encoder};

    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/rpb3_score_branch_properties.rs"
    ));

    mod rpb3_auxiliary_score_properties {
        use super::*;

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/rpb3_auxiliary_score_properties.rs"
        ));
    }

    mod cost_token_stack_rpb3_admission_differential {
        use super::*;

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/cost_token_stack_rpb3_admission_differential.rs"
        ));
    }

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

    fn authoritative_match_case_score(value: &MatchCase) -> Vec<u8> {
        let mut score = vec![2];
        score.extend(authoritative_par_score(
            value
                .pattern
                .as_ref()
                .expect("complete Match case has pattern"),
        ));
        score.extend(authoritative_par_score(
            value
                .source
                .as_ref()
                .expect("complete Match case has source"),
        ));
        score.extend([1, 1]);
        score.extend(((i64::from(value.free_count) as u64) ^ (1u64 << 63)).to_be_bytes());
        score.extend(authoritative_par_score(
            value.guard.as_ref().unwrap_or(&Par::default()),
        ));
        score.push(0);
        score
    }

    fn authoritative_receive_bind_score(value: &ReceiveBind) -> Option<Vec<u8>> {
        let source = value.source.as_ref()?;
        let mut score = vec![2];
        score.extend(authoritative_par_score(source));
        for pattern in &value.patterns {
            score.extend(authoritative_par_score(pattern));
        }
        score.extend(match &value.remainder {
            Some(remainder) => authoritative_var_score(remainder),
            None => authoritative_integer_atom(i64::from(Score::ABSENT)),
        });
        if let Some(region) = &value.cost_region {
            score.extend(authoritative_cost_region_score(region));
        }
        score.push(0);
        Some(score)
    }

    fn authoritative_integer_atom(value: i64) -> Vec<u8> {
        let mut score = vec![1, 1];
        score.extend(((value as u64) ^ (1u64 << 63)).to_be_bytes());
        score
    }

    fn authoritative_bytes_atom(value: &[u8]) -> Vec<u8> {
        let mut score = vec![1, 3];
        for &byte in value {
            score.push(byte);
            if byte == 0 {
                score.push(0xff);
            }
        }
        score.extend([0, 0]);
        score
    }

    fn authoritative_string_atom(value: &str) -> Vec<u8> {
        let mut score = vec![1, 2];
        for &byte in value.as_bytes() {
            score.push(byte);
            if byte == 0 {
                score.push(0xff);
            }
        }
        score.extend([0, 0]);
        score
    }

    fn authoritative_node(children: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
        let mut score = vec![2];
        for child in children {
            score.extend(child);
        }
        score.push(0);
        score
    }

    fn authoritative_constructor(constructor: i32, children: Vec<Vec<u8>>) -> Vec<u8> {
        authoritative_node(
            std::iter::once(authoritative_integer_atom(i64::from(constructor))).chain(children),
        )
    }

    fn authoritative_var_score(value: &Var) -> Vec<u8> {
        match value.var_instance {
            Some(VarInstance::BoundVar(level)) => authoritative_constructor(
                Score::BOUND_VAR,
                vec![authoritative_integer_atom(i64::from(level))],
            ),
            Some(VarInstance::FreeVar(level)) => {
                authoritative_constructor(Score::FREE_VAR, vec![authoritative_integer_atom(
                    i64::from(level),
                )])
            }
            Some(VarInstance::Wildcard(_)) => {
                authoritative_constructor(Score::WILDCARD, Vec::new())
            }
            None => authoritative_integer_atom(i64::from(Score::ABSENT)),
        }
    }

    fn authoritative_cost_region_score(value: &CostRegion) -> Vec<u8> {
        let mut children = vec![authoritative_integer_atom(i64::from(value.version))];
        for coordinate in [
            value.start_line,
            value.start_column,
            value.end_line,
            value.end_column,
        ] {
            children.push(authoritative_bytes_atom(&coordinate.to_be_bytes()));
        }
        children.push(match &value.signer {
            Some(signer) => authoritative_cost_signature_score(signer),
            None => authoritative_constructor(Score::COST_SIGNER_ABSENT, Vec::new()),
        });
        children.push(authoritative_integer_atom(i64::from(value.ordinal)));
        authoritative_constructor(Score::COST_REGION, children)
    }

    fn authoritative_cost_signature_atom_score(atom: &CostSignatureAtom) -> Vec<u8> {
        match atom.form.as_ref() {
            Some(Form::Ground(bytes)) => {
                authoritative_constructor(Score::COST_GROUND, vec![authoritative_bytes_atom(bytes)])
            }
            Some(Form::Quote(bytes)) => {
                authoritative_constructor(Score::COST_QUOTE, vec![authoritative_bytes_atom(bytes)])
            }
            Some(Form::RuntimeName(bytes)) => authoritative_constructor(
                Score::COST_RUNTIME_NAME,
                vec![authoritative_bytes_atom(bytes)],
            ),
            Some(Form::Bound(bound)) => {
                authoritative_constructor(Score::COST_BOUND, vec![authoritative_integer_atom(
                    i64::from(bound.index),
                )])
            }
            None => authoritative_constructor(Score::COST_FORM_ABSENT, Vec::new()),
        }
    }

    fn authoritative_cost_signature_score(value: &CostSignature) -> Vec<u8> {
        let atoms = value
            .atoms
            .iter()
            .map(authoritative_cost_signature_atom_score)
            .collect();
        authoritative_constructor(Score::COST_SIGNATURE, atoms)
    }

    fn property_var(level: i16, kind: u8) -> Var {
        let var_instance = match kind % 3 {
            0 => VarInstance::BoundVar(i32::from(level)),
            1 => VarInstance::FreeVar(i32::from(level)),
            _ => VarInstance::Wildcard(crate::rhoapi::var::WildcardMsg {}),
        };
        Var {
            var_instance: Some(var_instance),
        }
    }

    fn property_cost_region(seed: u16) -> CostRegion {
        CostRegion {
            version: u32::from(seed % 5),
            start_line: u64::from(seed),
            start_column: u64::from(seed % 13),
            end_line: u64::from(seed) + 1,
            end_column: u64::from(seed % 13) + 2,
            ordinal: u32::from(seed),
            ..CostRegion::default()
        }
    }

    fn property_signed_cost_region(seed: u16, atoms: Vec<CostSignatureAtom>) -> CostRegion {
        assert!(!atoms.is_empty());
        CostRegion {
            signer: Some(CostSignature { atoms }),
            ..property_cost_region(seed)
        }
    }

    fn property_signature_atoms(
        ground: Vec<u8>,
        quote: Vec<u8>,
        runtime_name: Vec<u8>,
        bound: i16,
    ) -> Vec<CostSignatureAtom> {
        assert!(!ground.is_empty());
        assert!(!quote.is_empty());
        assert!(!runtime_name.is_empty());
        vec![
            CostSignatureAtom {
                form: Some(Form::Ground(ground)),
            },
            CostSignatureAtom {
                form: Some(Form::Quote(quote)),
            },
            CostSignatureAtom {
                form: Some(Form::RuntimeName(runtime_name)),
            },
            CostSignatureAtom {
                form: Some(Form::Bound(CostBoundSignature {
                    index: i32::from(bound),
                })),
            },
            CostSignatureAtom::default(),
        ]
    }

    fn checked_aux_finish(
        bytes: &[u8],
        program: &'static RhoMatchProgram,
    ) -> (rpb3_root_finish_trace::RootFinishTrace, Vec<Vec<u8>>) {
        rpb3_root_finish_trace::start();
        let admission =
            validate_canonical_protobuf(ProtobufRegion::copy_from_slice(bytes), program)
                .unwrap_or_else(|error| {
                    panic!("property auxiliary input failed admission: {error}")
                });
        let trace = rpb3_root_finish_trace::take();
        rpb3_root_finish_trace::assert_driver_step_trace(&trace, program);
        let entries = trace
            .score_arena
            .as_ref()
            .expect("auxiliary property root captured its score arena");
        let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
        let returned = trace
            .returned
            .as_ref()
            .expect("auxiliary property returned");
        rpb3_root_finish_trace::assert_summary_score_source(
            returned,
            &transcripts,
            "auxiliary property",
        );
        assert_eq!(admission.component(), None);
        if returned.variant == rpb3_root_finish_trace::RootSemanticVariant::CostScore {
            assert!(admission.score_bytes().is_none());
        } else {
            assert_eq!(admission.score_bytes(), returned.score_bytes.as_deref());
        }
        (trace, transcripts)
    }

    fn check_no_score_child<T: Message + Default + PartialEq + std::fmt::Debug>(
        root_bytes: &[u8],
        root_program: &'static RhoMatchProgram,
        child: &T,
        child_program: &'static RhoMatchProgram,
        expected_child: rpb3_root_finish_trace::RootSemanticVariant,
        expected_payload: rpb3_root_finish_trace::NoScorePayload,
        expected_parent_complete: Option<bool>,
    ) -> rpb3_root_finish_trace::RootFinishTrace {
        let child_bytes = child.encode_to_vec();
        assert_eq!(
            &T::decode(child_bytes.as_slice()).expect("Prost child decoder"),
            child
        );
        rpb3_root_finish_trace::start();
        let admission =
            validate_canonical_protobuf(ProtobufRegion::copy_from_slice(root_bytes), root_program)
                .unwrap_or_else(|error| panic!("structural child input failed admission: {error}"));
        let trace = rpb3_root_finish_trace::take();
        let transcripts =
            rpb3_root_finish_trace::replay_score_source(trace.score_arena.as_ref().unwrap());
        assert_eq!(
            admission.component().map(|component| component.complete),
            expected_parent_complete
        );
        let eofs: Vec<_> = trace
            .child_eofs
            .iter()
            .filter(|event| std::ptr::eq(event.program, child_program) && event.body == child_bytes)
            .collect();
        let reduced: Vec<_> = trace
            .children
            .iter()
            .filter(|event| std::ptr::eq(event.program, child_program) && event.body == child_bytes)
            .collect();
        let pops: Vec<_> = trace
            .pop_accepts
            .iter()
            .filter(|event| {
                event
                    .child_program
                    .is_some_and(|program| std::ptr::eq(program, child_program))
                    && event.child_body.as_deref() == Some(child_bytes.as_slice())
            })
            .collect();
        assert_eq!(eofs.len(), 1);
        assert_eq!(reduced.len(), 1);
        assert_eq!(pops.len(), 1);
        let eof = eofs[0];
        let reduced = reduced[0];
        let pop = pops[0];
        assert_eq!(eof.frame, reduced.frame);
        assert_eq!(pop.child_frame.as_ref(), Some(&eof.frame));
        assert!(eof.frame.cursor_empty);
        assert!(!eof.frame.waiting);
        assert_eq!(eof.frame.region_len, child_bytes.len());
        assert_eq!(reduced.summary.variant, expected_child);
        assert_eq!(
            reduced.summary.no_score_payload.as_ref(),
            Some(&expected_payload)
        );
        assert_eq!(pop.summary.as_ref(), Some(&reduced.summary));
        assert_eq!(
            pop.summary.as_ref().map(|summary| summary.variant),
            Some(expected_child)
        );
        assert_eq!(
            pop.summary
                .as_ref()
                .and_then(|summary| summary.no_score_payload.as_ref()),
            Some(&expected_payload)
        );
        assert!(reduced.summary.score_handle.is_none());
        assert!(pop.summary.as_ref().unwrap().score_handle.is_none());
        assert!(reduced.summary.score_bytes.is_none());
        assert!(reduced.summary.component.is_none());
        assert_eq!(pop.source_len_before, eof.source_len);
        assert_eq!(pop.source_len_after, Some(eof.source_len));
        assert_eq!(pop.child_kind, FrameRequestKind::Message);
        assert_eq!(pop.before_len, 2);
        assert_eq!(pop.after_len, Some(1));
        assert_eq!(pop.parent_kind, Some(FrameRequestKind::Message));
        let parent_before = pop.parent_before.as_ref().unwrap();
        let parent_after = pop.parent_after.as_ref().unwrap();
        assert_eq!(parent_before.program_type_id, root_program.type_id);
        assert_eq!(parent_after.program_type_id, root_program.type_id);
        assert_eq!(parent_before.region_start, parent_after.region_start);
        assert!(parent_before.waiting);
        assert!(!parent_after.waiting);
        let returned = trace.returned.as_ref().unwrap();
        let expected_parent_variant = if std::ptr::eq(root_program, &VAR_RHO_MATCH_PROGRAM) {
            rpb3_root_finish_trace::RootSemanticVariant::Var
        } else if std::ptr::eq(root_program, &EMAP_RHO_MATCH_PROGRAM) {
            rpb3_root_finish_trace::RootSemanticVariant::ExprPayload
        } else if std::ptr::eq(root_program, &TAGGEDCONTINUATION_RHO_MATCH_PROGRAM)
            || std::ptr::eq(root_program, &LISTBINDPATTERNS_RHO_MATCH_PROGRAM)
        {
            rpb3_root_finish_trace::RootSemanticVariant::Opaque
        } else {
            rpb3_root_finish_trace::RootSemanticVariant::Component
        };
        assert_eq!(returned.variant, expected_parent_variant);
        assert_eq!(admission.score_bytes(), returned.score_bytes.as_deref());
        assert_eq!(trace.remaining_frames, Some(0));
        assert_eq!(
            returned
                .component
                .as_ref()
                .map(|component| component.complete),
            expected_parent_complete
        );
        let root_pop = trace.pop_accepts.last().unwrap();
        assert_eq!(root_pop.before_len, 1);
        assert_eq!(root_pop.after_len, Some(0));
        assert_eq!(root_pop.summary.as_ref(), Some(returned));
        if let Some(root_handle) = returned.score_handle.map(|handle| handle.index()) {
            assert_eq!(
                returned.score_bytes.as_deref(),
                Some(transcripts[root_handle].as_slice())
            );
        } else {
            assert!(returned.score_bytes.is_none());
        }
        if expected_parent_complete == Some(false) {
            let root_handle = returned.score_handle.unwrap().index();
            assert_eq!(
                transcripts[root_handle],
                authoritative_constructor(i32::MAX, Vec::new())
            );
        }
        trace
    }

    fn assert_direct_child_descriptor(
        parent: &'static RhoMatchProgram,
        child: &'static RhoMatchProgram,
        tag: u32,
    ) {
        assert!(parent.fields.iter().any(|field| match field.kind {
            RhoMatchFieldKind::OptionalMessage(type_id)
            | RhoMatchFieldKind::MessageSequence(type_id) => {
                field.tag == tag && type_id == child.type_id
            }
            RhoMatchFieldKind::Oneof(arms) => arms.iter().any(|arm| {
                arm.tag == tag
                    && matches!(arm.payload, RhoMatchPayloadKind::Message(type_id) if type_id == child.type_id)
            }),
            _ => false,
        }));
    }

    fn assert_nested_child_score(
        trace: &rpb3_root_finish_trace::RootFinishTrace,
        program: &'static RhoMatchProgram,
        body: &[u8],
        expected_score: &[u8],
    ) {
        let matches: Vec<_> = trace
            .children
            .iter()
            .filter(|child| std::ptr::eq(child.program, program) && child.body == body)
            .collect();
        assert_eq!(matches.len(), 1);
        let child = matches[0];
        assert_eq!(child.summary.score_bytes.as_deref(), Some(expected_score));
        let transcripts =
            rpb3_root_finish_trace::replay_score_source(trace.score_arena.as_ref().unwrap());
        rpb3_root_finish_trace::assert_summary_score_source(
            &child.summary,
            &transcripts,
            "nested structural child score",
        );
    }

    fn generated_prost_root_bytes<
        T: Message + ProtobufNode + Default + PartialEq + std::fmt::Debug,
    >(
        value: &T,
        generated_decode: impl Fn(&[u8]) -> T,
    ) -> Vec<u8> {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let prost = T::decode(bytes.as_slice()).expect("Prost root decoder");
        assert_eq!(generated_decode(bytes.as_slice()), prost);
        assert_eq!(&prost, value);
        bytes
    }

    fn expected_expr_source_constructor(
        entries: &mut Vec<CanonicalScoreSourceKind>,
        before: usize,
        constructor: i32,
        children: &[usize],
    ) {
        let constructor_index = before + entries.len();
        entries.push(CanonicalScoreSourceKind::Integer(i64::from(constructor)));
        entries.push(CanonicalScoreSourceKind::Node(
            std::iter::once(constructor_index)
                .chain(children.iter().copied())
                .collect(),
        ));
    }

    struct SelectedExprPayloadOracle {
        program: &'static RhoMatchProgram,
        body: Vec<u8>,
        source_child_program: &'static RhoMatchProgram,
        source_children: Vec<(Vec<u8>, Vec<u8>)>,
        constructor: Option<i32>,
        complete: bool,
    }

    fn par_expr_source_children<'a>(
        pars: impl IntoIterator<Item = &'a Par>,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        pars.into_iter()
            .map(|par| {
                (
                    protobuf_encoder::encode_to_vec(par),
                    authoritative_par_score(par),
                )
            })
            .collect()
    }

    fn selected_binary_expr_payload(instance: &ExprInstance) -> Option<SelectedExprPayloadOracle> {
        macro_rules! binary {
            ($value:expr, $program:ident, $constructor:expr, $first:ident, $second:ident) => {{
                let value = $value;
                Some(SelectedExprPayloadOracle {
                    program: &$program,
                    body: protobuf_encoder::encode_to_vec(value),
                    source_child_program: &PAR_PROTOBUF_REGION_PROGRAM,
                    source_children: par_expr_source_children(
                        value.$first.iter().chain(value.$second.iter()),
                    ),
                    constructor: Some($constructor),
                    complete: value.$first.is_some() && value.$second.is_some(),
                })
            }};
        }
        match instance {
            ExprInstance::EMultBody(value) => {
                binary!(value, EMULT_RHO_MATCH_PROGRAM, Score::EMULT, p1, p2)
            }
            ExprInstance::EDivBody(value) => {
                binary!(value, EDIV_RHO_MATCH_PROGRAM, Score::EDIV, p1, p2)
            }
            ExprInstance::EModBody(value) => {
                binary!(value, EMOD_RHO_MATCH_PROGRAM, Score::EMOD, p1, p2)
            }
            ExprInstance::EPlusBody(value) => {
                binary!(value, EPLUS_RHO_MATCH_PROGRAM, Score::EPLUS, p1, p2)
            }
            ExprInstance::EMinusBody(value) => {
                binary!(value, EMINUS_RHO_MATCH_PROGRAM, Score::EMINUS, p1, p2)
            }
            ExprInstance::ELtBody(value) => {
                binary!(value, ELT_RHO_MATCH_PROGRAM, Score::ELT, p1, p2)
            }
            ExprInstance::ELteBody(value) => {
                binary!(value, ELTE_RHO_MATCH_PROGRAM, Score::ELTE, p1, p2)
            }
            ExprInstance::EGtBody(value) => {
                binary!(value, EGT_RHO_MATCH_PROGRAM, Score::EGT, p1, p2)
            }
            ExprInstance::EGteBody(value) => {
                binary!(value, EGTE_RHO_MATCH_PROGRAM, Score::EGTE, p1, p2)
            }
            ExprInstance::EEqBody(value) => {
                binary!(value, EEQ_RHO_MATCH_PROGRAM, Score::EEQ, p1, p2)
            }
            ExprInstance::ENeqBody(value) => {
                binary!(value, ENEQ_RHO_MATCH_PROGRAM, Score::ENEQ, p1, p2)
            }
            ExprInstance::EAndBody(value) => {
                binary!(value, EAND_RHO_MATCH_PROGRAM, Score::EAND, p1, p2)
            }
            ExprInstance::EOrBody(value) => {
                binary!(value, EOR_RHO_MATCH_PROGRAM, Score::EOR, p1, p2)
            }
            ExprInstance::EMatchesBody(value) => binary!(
                value,
                EMATCHES_RHO_MATCH_PROGRAM,
                Score::EMATCHES,
                target,
                pattern
            ),
            ExprInstance::EPercentPercentBody(value) => binary!(
                value,
                EPERCENTPERCENT_RHO_MATCH_PROGRAM,
                Score::EPERCENT,
                p1,
                p2
            ),
            ExprInstance::EPlusPlusBody(value) => {
                binary!(value, EPLUSPLUS_RHO_MATCH_PROGRAM, Score::EPLUSPLUS, p1, p2)
            }
            ExprInstance::EMinusMinusBody(value) => binary!(
                value,
                EMINUSMINUS_RHO_MATCH_PROGRAM,
                Score::EMINUSMINUS,
                p1,
                p2
            ),
            _ => None,
        }
    }

    fn selected_expr_payload(instance: &ExprInstance) -> Option<SelectedExprPayloadOracle> {
        match instance {
            ExprInstance::EVarBody(value) => Some(SelectedExprPayloadOracle {
                program: &EVAR_RHO_MATCH_PROGRAM,
                body: protobuf_encoder::encode_to_vec(value),
                source_child_program: &VAR_RHO_MATCH_PROGRAM,
                source_children: value
                    .v
                    .iter()
                    .map(|var| {
                        (
                            protobuf_encoder::encode_to_vec(var),
                            authoritative_var_score(var),
                        )
                    })
                    .collect(),
                constructor: Some(Score::EVAR),
                complete: value.v.is_some(),
            }),
            ExprInstance::ENegBody(value) => Some(SelectedExprPayloadOracle {
                program: &ENEG_RHO_MATCH_PROGRAM,
                body: protobuf_encoder::encode_to_vec(value),
                source_child_program: &PAR_PROTOBUF_REGION_PROGRAM,
                source_children: par_expr_source_children(value.p.iter()),
                constructor: Some(Score::ENEG),
                complete: value.p.is_some(),
            }),
            ExprInstance::ENotBody(value) => Some(SelectedExprPayloadOracle {
                program: &ENOT_RHO_MATCH_PROGRAM,
                body: protobuf_encoder::encode_to_vec(value),
                source_child_program: &PAR_PROTOBUF_REGION_PROGRAM,
                source_children: par_expr_source_children(value.p.iter()),
                constructor: Some(Score::ENOT),
                complete: value.p.is_some(),
            }),
            ExprInstance::EMethodBody(value) if value.target.is_none() => {
                Some(SelectedExprPayloadOracle {
                    program: &EMETHOD_RHO_MATCH_PROGRAM,
                    body: protobuf_encoder::encode_to_vec(value),
                    source_child_program: &PAR_PROTOBUF_REGION_PROGRAM,
                    source_children: par_expr_source_children(value.arguments.iter()),
                    constructor: None,
                    complete: false,
                })
            }
            ExprInstance::EZipperBody(value) if value.pathmap.is_none() => {
                Some(SelectedExprPayloadOracle {
                    program: &EZIPPER_RHO_MATCH_PROGRAM,
                    body: protobuf_encoder::encode_to_vec(value),
                    source_child_program: &PAR_PROTOBUF_REGION_PROGRAM,
                    source_children: Vec::new(),
                    constructor: None,
                    complete: false,
                })
            }
            _ => selected_binary_expr_payload(instance),
        }
    }

    fn expected_expr_finish_source(
        oracle: &Expr,
        before: usize,
    ) -> (
        Vec<CanonicalScoreSourceKind>,
        Vec<u8>,
        bool,
        bool,
        Option<&'static RhoMatchProgram>,
    ) {
        let mut entries = Vec::new();
        let mut child_program = None;
        let (score, complete, stable) = match oracle.expr_instance.as_ref() {
            None => (
                authoritative_constructor(Score::ABSENT, Vec::new()),
                true,
                false,
            ),
            Some(ExprInstance::GBool(value)) => {
                let atom = i64::from(!*value);
                entries.push(CanonicalScoreSourceKind::Integer(atom));
                (
                    authoritative_constructor(Score::BOOL, vec![authoritative_integer_atom(atom)]),
                    true,
                    true,
                )
            }
            Some(ExprInstance::GInt(value)) => {
                entries.push(CanonicalScoreSourceKind::Integer(*value));
                (
                    authoritative_constructor(Score::INT, vec![authoritative_integer_atom(*value)]),
                    true,
                    true,
                )
            }
            Some(ExprInstance::GString(value)) | Some(ExprInstance::GUri(value)) => {
                let constructor = if matches!(
                    oracle.expr_instance.as_ref(),
                    Some(ExprInstance::GString(_))
                ) {
                    Score::STRING
                } else {
                    Score::URI
                };
                entries.push(CanonicalScoreSourceKind::StringIndexed(
                    value.as_bytes().to_vec(),
                ));
                (
                    authoritative_constructor(constructor, vec![authoritative_string_atom(value)]),
                    true,
                    true,
                )
            }
            Some(ExprInstance::GByteArray(value)) | Some(ExprInstance::GBigInt(value)) => {
                let constructor = if matches!(
                    oracle.expr_instance.as_ref(),
                    Some(ExprInstance::GByteArray(_))
                ) {
                    Score::EBYTEARR
                } else {
                    Score::BIG_INT
                };
                entries.push(CanonicalScoreSourceKind::BytesIndexed(value.clone()));
                (
                    authoritative_constructor(constructor, vec![authoritative_bytes_atom(value)]),
                    true,
                    true,
                )
            }
            Some(ExprInstance::GDouble(value)) => {
                let atom = *value as i64;
                entries.push(CanonicalScoreSourceKind::Integer(atom));
                (
                    authoritative_constructor(Score::DOUBLE, vec![authoritative_integer_atom(
                        atom,
                    )]),
                    true,
                    true,
                )
            }
            Some(ExprInstance::GBigRat(value)) => {
                entries.push(CanonicalScoreSourceKind::BytesIndexed(
                    value.numerator.clone(),
                ));
                entries.push(CanonicalScoreSourceKind::BytesIndexed(
                    value.denominator.clone(),
                ));
                (
                    authoritative_constructor(Score::BIG_RAT, vec![
                        authoritative_bytes_atom(&value.numerator),
                        authoritative_bytes_atom(&value.denominator),
                    ]),
                    true,
                    true,
                )
            }
            Some(ExprInstance::GFixedPoint(value)) => {
                entries.push(CanonicalScoreSourceKind::BytesIndexed(
                    value.unscaled.clone(),
                ));
                let scale_index = before + entries.len();
                entries.push(CanonicalScoreSourceKind::Integer(i64::from(value.scale)));
                entries.push(CanonicalScoreSourceKind::Node(vec![scale_index]));
                (
                    authoritative_constructor(Score::FIXED_POINT, vec![
                        authoritative_bytes_atom(&value.unscaled),
                        authoritative_node([authoritative_integer_atom(i64::from(value.scale))]),
                    ]),
                    true,
                    true,
                )
            }
            Some(instance) => {
                let payload = selected_expr_payload(instance)
                    .expect("Expr finish source oracle received a selected payload");
                child_program = Some(payload.program);
                (
                    if payload.complete {
                        authoritative_constructor(
                            payload
                                .constructor
                                .expect("complete payload has a constructor"),
                            payload
                                .source_children
                                .into_iter()
                                .map(|(_, score)| score)
                                .collect(),
                        )
                    } else {
                        authoritative_constructor(
                            semantics::NONSEMANTIC_SCORE_CONSTRUCTOR,
                            Vec::new(),
                        )
                    },
                    payload.complete,
                    false,
                )
            }
        };
        if child_program.is_none() || !complete {
            let constructor = if child_program.is_some() {
                semantics::NONSEMANTIC_SCORE_CONSTRUCTOR
            } else {
                match oracle.expr_instance.as_ref() {
                    None => Score::ABSENT,
                    Some(ExprInstance::GBool(_)) => Score::BOOL,
                    Some(ExprInstance::GInt(_)) => Score::INT,
                    Some(ExprInstance::GString(_)) => Score::STRING,
                    Some(ExprInstance::GUri(_)) => Score::URI,
                    Some(ExprInstance::GByteArray(_)) => Score::EBYTEARR,
                    Some(ExprInstance::GDouble(_)) => Score::DOUBLE,
                    Some(ExprInstance::GBigInt(_)) => Score::BIG_INT,
                    Some(ExprInstance::GBigRat(_)) => Score::BIG_RAT,
                    Some(ExprInstance::GFixedPoint(_)) => Score::FIXED_POINT,
                    _ => panic!("Expr source constructor oracle received a payload"),
                }
            };
            let child_indices: Vec<_> = (before..before + entries.len())
                .filter(|index| {
                    !matches!(
                        oracle.expr_instance.as_ref(),
                        Some(ExprInstance::GFixedPoint(_))
                    ) || *index == before
                        || *index == before + 2
                })
                .collect();
            expected_expr_source_constructor(&mut entries, before, constructor, &child_indices);
        }
        (entries, score, complete, stable, child_program)
    }

    fn check_generated_expr_finish_source(expr: &Expr) {
        let bytes = generated_prost_root_bytes(expr, |bytes| {
            protobuf_decoder::decode_expr(bytes).expect("generated Expr decoder")
        });
        let oracle = Expr::decode(bytes.as_slice()).expect("Prost Expr decoder");
        rpb3_root_finish_trace::start();
        let admission = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes),
            &EXPR_RHO_MATCH_PROGRAM,
        )
        .expect("generated Expr finish input is admitted");
        let trace = rpb3_root_finish_trace::take();
        let finishes: Vec<_> = trace
            .expr_finishes
            .iter()
            .filter(|finish| finish.body == bytes)
            .collect();
        assert_eq!(finishes.len(), 1);
        let finish = finishes[0];
        let (expected_entries, expected_score, complete, stable, child_program) =
            expected_expr_finish_source(&oracle, finish.source_len_before);
        assert_eq!(finish.complete, complete);
        assert_eq!(finish.canonical_path_stable, stable);
        assert_eq!(
            finish.source_len_after,
            finish.source_len_before + expected_entries.len()
        );
        let entries = trace
            .score_arena
            .as_ref()
            .expect("Expr score arena recorded");
        let actual_entries: Vec<_> = entries[finish.source_len_before..finish.source_len_after]
            .iter()
            .map(|entry| entry.kind.clone())
            .collect();
        assert_eq!(actual_entries, expected_entries);
        let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
        assert_eq!(transcripts[finish.score_handle.index()], expected_score);
        assert_eq!(admission.score_bytes(), Some(expected_score.as_slice()));
        let component = admission
            .component()
            .expect("Expr admission returns component");
        assert_eq!(component.family, ParComponentTag::Expr);
        assert_eq!(component.complete, complete);
        assert_eq!(component.canonical_path_stable, stable);
        let returned = trace.returned.as_ref().expect("Expr returns a summary");
        assert_eq!(returned.score_handle, Some(finish.score_handle));
        let returned_component = returned.component.as_ref().expect("Expr component summary");
        assert_eq!(returned_component.family, ParComponentTag::Expr);
        assert_eq!(returned_component.complete, complete);
        assert_eq!(returned_component.canonical_path_stable, stable);
        rpb3_root_finish_trace::assert_summary_score_source(
            returned,
            &transcripts,
            "generated Expr finish",
        );
        if let Some(program) = child_program {
            let selected = selected_expr_payload(oracle.expr_instance.as_ref().unwrap()).unwrap();
            assert!(std::ptr::eq(program, selected.program));
            let body = &selected.body;
            let children: Vec<_> = trace
                .children
                .iter()
                .filter(|child| std::ptr::eq(child.program, program) && child.body == *body)
                .collect();
            assert_eq!(children.len(), 1);
            let child = children[0];
            assert_eq!(
                child.summary.variant,
                rpb3_root_finish_trace::RootSemanticVariant::ExprPayload
            );
            if complete {
                assert_eq!(child.summary.score_handle, Some(finish.score_handle));
                assert_eq!(
                    child.summary.score_bytes.as_deref(),
                    Some(expected_score.as_slice())
                );
            } else {
                assert!(child.summary.score_handle.is_none());
                assert_eq!(
                    child.summary.no_score_payload,
                    Some(rpb3_root_finish_trace::NoScorePayload::ExprPayloadAbsent)
                );
            }
            let source_children_actual: Vec<_> = trace
                .children
                .iter()
                .filter(|child| std::ptr::eq(child.program, selected.source_child_program))
                .collect();
            assert_eq!(source_children_actual.len(), selected.source_children.len());
            let mut source_child_handles = Vec::new();
            for (actual, (expected_body, expected_score)) in
                source_children_actual.iter().zip(selected.source_children)
            {
                assert_eq!(actual.body, expected_body);
                assert_eq!(
                    actual.summary.score_bytes.as_deref(),
                    Some(expected_score.as_slice())
                );
                source_child_handles.push(actual.summary.score_handle.unwrap().index());
            }
            let payload_finishes: Vec<_> = trace
                .expr_payload_finishes
                .iter()
                .filter(|payload| std::ptr::eq(payload.program, program) && payload.body == *body)
                .collect();
            assert_eq!(payload_finishes.len(), 1);
            let payload = payload_finishes[0];
            let payload_before = payload.source_len_before;
            if complete {
                let constructor = selected.constructor.unwrap();
                assert_eq!(payload.source_len_after, payload_before + 2);
                assert_eq!(
                    entries[payload_before].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(constructor))
                );
                assert_eq!(
                    entries[payload_before + 1].kind,
                    CanonicalScoreSourceKind::Node(
                        std::iter::once(payload_before)
                            .chain(source_child_handles)
                            .collect()
                    )
                );
                assert_eq!(payload.score_handle, Some(finish.score_handle));
                assert_eq!(finish.source_len_before, payload.source_len_after);
            } else {
                assert_eq!(payload.source_len_after, payload_before);
                assert!(payload.score_handle.is_none());
                assert_eq!(finish.source_len_before, payload_before);
            }
        } else {
            assert_eq!(finish.score_handle.index() + 1, finish.source_len_after);
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum GeneratedEMapKeyIdentity {
        Empty,
        Int(i64),
        Composite(i64, i64),
        NestedMap { key: i64, value: i64 },
    }

    fn generated_emap_key(identity: GeneratedEMapKeyIdentity, locally_free: &[u8]) -> Par {
        let key = match identity {
            GeneratedEMapKeyIdentity::Empty => Par::default(),
            GeneratedEMapKeyIdentity::Int(value) => gint(value),
            GeneratedEMapKeyIdentity::Composite(first, second) => Par::default().with_exprs(vec![
                Expr {
                    expr_instance: Some(ExprInstance::GInt(first)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GInt(second)),
                },
            ]),
            GeneratedEMapKeyIdentity::NestedMap { key, value } => {
                Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::EMapBody(EMap {
                        kvs: vec![KeyValuePair {
                            key: Some(gint(key)),
                            value: Some(gint(value)),
                        }],
                        ..EMap::default()
                    })),
                }])
            }
        };
        key.with_locally_free(locally_free.to_vec())
    }

    fn sort_generated_emap_pairs(pairs: &mut [(KeyValuePair, Option<GeneratedEMapKeyIdentity>)]) {
        pairs.sort_by(|(left, _), (right, _)| {
            (
                authoritative_par_score(left.key.as_ref().unwrap()),
                protobuf_encoder::encode_to_vec(left),
            )
                .cmp(&(
                    authoritative_par_score(right.key.as_ref().unwrap()),
                    protobuf_encoder::encode_to_vec(right),
                ))
        });
    }

    fn check_generated_emap_finish_source(
        map: &EMap,
        key_identities: &[Option<GeneratedEMapKeyIdentity>],
        expected_complete: bool,
        expected_fixed: bool,
    ) {
        let bytes = generated_prost_root_bytes(map, |bytes| {
            protobuf_decoder::decode_e_map(bytes).expect("generated EMap decoder")
        });
        let oracle = EMap::decode(bytes.as_slice()).expect("Prost EMap decoder");
        let mut seen_identities = BTreeSet::new();
        let mut previous_order: Option<(Vec<u8>, Vec<u8>)> = None;
        let mut complete = true;
        let mut fixed = true;
        let mut scored_keys = Vec::new();
        let mut pair_bodies = Vec::new();
        assert_eq!(oracle.kvs.len(), key_identities.len());
        for (pair, expected_identity) in oracle.kvs.iter().zip(key_identities) {
            let pair_body = protobuf_encoder::encode_to_vec(pair);
            assert_eq!(KeyValuePair::decode(pair_body.as_slice()).unwrap(), *pair);
            if let Some(identity) = expected_identity {
                let key = pair.key.as_ref().expect("identity oracle requires a key");
                let expected_key = generated_emap_key(*identity, key.locally_free());
                assert_eq!(
                    protobuf_encoder::encode_to_vec(key),
                    protobuf_encoder::encode_to_vec(&expected_key)
                );
            } else {
                assert!(pair.key.is_none());
            }
            if let (Some(key), Some(_)) = (&pair.key, &pair.value) {
                let key_score = authoritative_par_score(key);
                let ordering = (key_score.clone(), pair_body.clone());
                if let Some(previous) = previous_order.as_ref() {
                    fixed &= previous < &ordering;
                }
                fixed &= seen_identities.insert(expected_identity.unwrap());
                previous_order = Some(ordering);
                scored_keys.push(key_score);
            } else {
                complete = false;
            }
            pair_bodies.push(pair_body);
        }
        assert_eq!(complete, expected_complete);
        assert_eq!(fixed, expected_fixed);

        rpb3_root_finish_trace::start();
        let admission = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes),
            &EMAP_RHO_MATCH_PROGRAM,
        )
        .expect("generated direct EMap admission");
        let trace = rpb3_root_finish_trace::take();
        let finish = trace
            .emap_finishes
            .iter()
            .find(|finish| finish.body == bytes)
            .expect("root EMap finish receipt");
        assert_eq!(finish.body, bytes);
        assert_eq!(finish.complete, complete);
        assert_eq!(finish.canonical_fixed_point, fixed);
        let entries = trace.score_arena.as_ref().unwrap();
        let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
        let pair_pops: Vec<_> = trace
            .pop_accepts
            .iter()
            .filter(|pop| {
                pop.child_program
                    .is_some_and(|program| std::ptr::eq(program, &KEYVALUEPAIR_RHO_MATCH_PROGRAM))
                    && pop.parent_before.as_ref().is_some_and(|parent| {
                        parent.program_type_id == EMAP_RHO_MATCH_PROGRAM.type_id
                            && parent.region_start == CanonicalByteProvenance::default()
                            && parent.region_len == bytes.len()
                    })
            })
            .collect();
        assert_eq!(pair_pops.len(), oracle.kvs.len());
        for (pop, (pair, pair_body)) in pair_pops.iter().zip(oracle.kvs.iter().zip(&pair_bodies)) {
            let child = trace
                .children
                .iter()
                .find(|child| {
                    std::ptr::eq(child.program, &KEYVALUEPAIR_RHO_MATCH_PROGRAM)
                        && pop.child_frame.as_ref() == Some(&child.frame)
                        && child.body == *pair_body
                })
                .expect("outer EMap pair child finish");
            assert_eq!(pop.child_body.as_deref(), Some(pair_body.as_slice()));
            assert_eq!(pop.summary.as_ref(), Some(&child.summary));
            assert_eq!(pop.source_len_after, Some(pop.source_len_before));
            assert_eq!(child.summary.score_handle, None);
            assert_eq!(
                child.summary.no_score_payload,
                Some(rpb3_root_finish_trace::NoScorePayload::KeyValuePair {
                    key: pair.key.as_ref().map(authoritative_par_score),
                    value: pair.value.as_ref().map(authoritative_par_score),
                })
            );
        }
        assert_eq!(finish.pair_scores.len(), scored_keys.len());
        for (handle, expected) in finish.pair_scores.iter().zip(&scored_keys) {
            assert_eq!(transcripts[handle.index()], *expected);
        }
        let returned = trace.returned.as_ref().unwrap();
        rpb3_root_finish_trace::assert_summary_score_source(returned, &transcripts, "direct EMap");
        assert_eq!(
            returned.variant,
            rpb3_root_finish_trace::RootSemanticVariant::ExprPayload
        );
        if complete {
            let before = finish.source_len_before;
            assert_eq!(finish.source_len_after, before + 4);
            assert_eq!(entries[before].kind, CanonicalScoreSourceKind::Integer(-1));
            assert_eq!(
                entries[before + 1].kind,
                CanonicalScoreSourceKind::Integer(i64::from(oracle.connective_used))
            );
            assert_eq!(
                entries[before + 2].kind,
                CanonicalScoreSourceKind::Integer(i64::from(Score::EMAP))
            );
            let remainder_handle = if let Some(remainder) = &oracle.remainder {
                let remainder_body = protobuf_encoder::encode_to_vec(remainder);
                let child = trace
                    .children
                    .iter()
                    .find(|child| {
                        std::ptr::eq(child.program, &VAR_RHO_MATCH_PROGRAM)
                            && child.body == remainder_body
                    })
                    .expect("present EMap remainder child");
                assert_eq!(
                    child.summary.score_bytes.as_deref(),
                    Some(authoritative_var_score(remainder).as_slice())
                );
                child.summary.score_handle.unwrap().index()
            } else {
                before
            };
            let mut node_children = vec![before + 2, remainder_handle];
            node_children.extend(finish.pair_scores.iter().map(|handle| handle.index()));
            node_children.push(before + 1);
            assert_eq!(
                entries[before + 3].kind,
                CanonicalScoreSourceKind::Node(node_children)
            );
            assert_eq!(finish.score_handle.unwrap().index(), before + 3);
            let mut score_children = vec![match &oracle.remainder {
                Some(remainder) => authoritative_var_score(remainder),
                None => authoritative_integer_atom(-1),
            }];
            score_children.extend(scored_keys);
            score_children.push(authoritative_integer_atom(i64::from(
                oracle.connective_used,
            )));
            let expected_score = authoritative_constructor(Score::EMAP, score_children);
            assert_eq!(transcripts[before + 3], expected_score);
            assert_eq!(admission.score_bytes(), Some(expected_score.as_slice()));
            assert_eq!(returned.score_handle, finish.score_handle);
        } else {
            assert_eq!(finish.source_len_after, finish.source_len_before);
            assert!(finish.score_handle.is_none());
            assert!(returned.score_handle.is_none());
            assert!(admission.score_bytes().is_none());
        }

        let expr = Expr {
            expr_instance: Some(ExprInstance::EMapBody(map.clone())),
        };
        let expr_bytes = generated_prost_root_bytes(&expr, |bytes| {
            protobuf_decoder::decode_expr(bytes).expect("generated EMap Expr decoder")
        });
        rpb3_root_finish_trace::start();
        let expr_result = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&expr_bytes),
            &EXPR_RHO_MATCH_PROGRAM,
        );
        let expr_trace = rpb3_root_finish_trace::take();
        let expr_map_finish = expr_trace
            .emap_finishes
            .iter()
            .find(|finish| finish.body == bytes)
            .expect("Expr child EMap finish");
        if complete && !fixed {
            assert!(expr_result.is_err());
            assert!(!expr_map_finish.canonical_fixed_point);
            assert!(expr_trace.returned.is_none());
        } else {
            let expr_admission = expr_result.expect("canonical or incomplete EMap Expr admits");
            assert_eq!(expr_admission.component().unwrap().complete, complete);
            let expected_score = if complete {
                admission.score_bytes().unwrap().to_vec()
            } else {
                authoritative_constructor(semantics::NONSEMANTIC_SCORE_CONSTRUCTOR, Vec::new())
            };
            assert_eq!(
                expr_admission.score_bytes(),
                Some(expected_score.as_slice())
            );
            let expr_entries = expr_trace.score_arena.as_ref().unwrap();
            let expr_transcripts = rpb3_root_finish_trace::replay_score_source(expr_entries);
            let root_finish = expr_trace
                .expr_finishes
                .iter()
                .find(|finish| finish.body == expr_bytes)
                .expect("EMap Expr root finish");
            assert_eq!(
                expr_transcripts[root_finish.score_handle.index()],
                expected_score
            );
            if complete {
                assert_eq!(root_finish.source_len_before, root_finish.source_len_after);
                assert_eq!(
                    root_finish.score_handle,
                    expr_map_finish.score_handle.unwrap()
                );
            } else {
                let before = root_finish.source_len_before;
                assert_eq!(root_finish.source_len_after, before + 2);
                assert_eq!(
                    expr_entries[before].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(
                        semantics::NONSEMANTIC_SCORE_CONSTRUCTOR
                    ))
                );
                assert_eq!(
                    expr_entries[before + 1].kind,
                    CanonicalScoreSourceKind::Node(vec![before])
                );
            }
        }
    }

    fn independent_epathmap_snapshot_field(bytes: &[u8]) -> Vec<u8> {
        fn varint(bytes: &[u8], offset: &mut usize) -> u64 {
            let mut value = 0u64;
            for shift in (0..70).step_by(7) {
                let byte = *bytes.get(*offset).expect("generated protobuf varint byte");
                *offset += 1;
                value |= u64::from(byte & 0x7f) << shift;
                if byte & 0x80 == 0 {
                    return value;
                }
            }
            panic!("generated protobuf varint is bounded");
        }

        let mut offset = 0;
        let mut snapshot = None;
        while offset < bytes.len() {
            let key = varint(bytes, &mut offset);
            let tag = key >> 3;
            let wire = key & 7;
            if wire == 0 {
                varint(bytes, &mut offset);
            } else {
                assert_eq!(wire, 2);
                let len = usize::try_from(varint(bytes, &mut offset)).unwrap();
                let end = offset.checked_add(len).unwrap();
                let body = bytes.get(offset..end).expect("generated field body");
                if tag == 9 {
                    assert!(snapshot.replace(body.to_vec()).is_none());
                }
                offset = end;
            }
        }
        snapshot.expect("generated EPathMap contains tag 9")
    }

    fn checked_property_finish<T: ParBagComponent>(
        value: &T,
        bytes: &[u8],
        program: &'static RhoMatchProgram,
        expected_complete: bool,
    ) -> (rpb3_root_finish_trace::RootFinishTrace, Vec<Vec<u8>>) {
        assert_eq!(T::semantic_shape_is_complete(value), expected_complete);
        assert!(is_authoritative_component_body(value, bytes));
        rpb3_root_finish_trace::start();
        let admission =
            validate_canonical_protobuf(ProtobufRegion::copy_from_slice(bytes), program)
                .unwrap_or_else(|error| panic!("property input failed admission: {error}"));
        let trace = rpb3_root_finish_trace::take();
        rpb3_root_finish_trace::assert_driver_step_trace(&trace, program);
        let entries = trace
            .score_arena
            .as_ref()
            .expect("property root return captured its score arena");
        let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
        let returned = trace
            .returned
            .as_ref()
            .expect("property root returned a score");
        rpb3_root_finish_trace::assert_summary_score_source(returned, &transcripts, "property");
        assert_eq!(
            admission.component(),
            Some(CanonicalComponentAdmission {
                family: T::TAG,
                complete: expected_complete,
                canonical_path_stable: T::canonical_path_stable(value),
            })
        );
        assert_eq!(
            returned
                .component
                .as_ref()
                .map(|component| component.complete),
            Some(expected_complete)
        );
        assert_eq!(returned.score_bytes.as_deref(), admission.score_bytes());
        with_component_key(value, |key| {
            let trailer = key.len() - std::mem::size_of::<u64>();
            let score_len = usize::try_from(u64::from_be_bytes(
                key[trailer..].try_into().expect("RPB3 score length word"),
            ))
            .expect("property score length fits usize");
            let score_start = PAR_BAG_KEY_DOMAIN.len() + 1;
            let score_end = score_start + score_len;
            assert_eq!(admission.score_bytes(), Some(&key[score_start..score_end]));
            assert_eq!(&key[score_end..trailer], bytes);
        });
        (trace, transcripts)
    }

    fn check_send_finish_property(value: &Send, complete: bool) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let decoded =
            protobuf_decoder::decode_send(bytes.as_slice()).expect("generated Send decoder");
        let oracle = Send::decode(bytes.as_slice()).expect("Prost Send decoder");
        assert_eq!(decoded, oracle);
        assert_eq!(oracle.encode_to_vec(), bytes);
        let (trace, transcripts) =
            checked_property_finish(value, &bytes, &SEND_RHO_MATCH_PROGRAM, complete);
        assert_eq!(trace.send_finishes.len(), 1);
        let finish = &trace.send_finishes[0];
        assert_eq!(finish.body, bytes);
        assert_eq!(finish.complete, complete);
        assert_eq!(
            trace.returned.as_ref().unwrap().score_handle,
            Some(finish.score_handle)
        );
        rpb3_root_finish_trace::assert_send_finish_source(
            finish,
            trace.score_arena.as_ref().unwrap(),
            &transcripts,
            "Send property",
        );
        match &finish.input {
            SendFinishInput::Incomplete => assert!(oracle.chan.is_none()),
            SendFinishInput::Complete {
                persistent,
                channel,
                data,
                connective_used,
                cost,
            } => {
                assert_eq!(*persistent, oracle.persistent);
                assert_eq!(*connective_used, oracle.connective_used);
                assert_eq!(cost.is_some(), oracle.cost_region.is_some());
                assert_eq!(data.len(), oracle.data.len());
                assert_eq!(
                    transcripts[channel.index()],
                    authoritative_par_score(oracle.chan.as_ref().unwrap())
                );
                for (handle, child) in data.iter().zip(&oracle.data) {
                    assert_eq!(transcripts[handle.index()], authoritative_par_score(child));
                }
            }
        }
    }

    fn check_bundle_finish_property(value: &Bundle, complete: bool) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let decoded =
            protobuf_decoder::decode_bundle(bytes.as_slice()).expect("generated Bundle decoder");
        let oracle = Bundle::decode(bytes.as_slice()).expect("Prost Bundle decoder");
        assert_eq!(decoded, oracle);
        assert_eq!(oracle.encode_to_vec(), bytes);
        let (trace, transcripts) =
            checked_property_finish(value, &bytes, &BUNDLE_RHO_MATCH_PROGRAM, complete);
        assert_eq!(trace.bundle_finishes.len(), 1);
        let finish = &trace.bundle_finishes[0];
        assert_eq!(finish.body, bytes);
        assert_eq!(finish.complete, complete);
        assert_eq!(
            trace.returned.as_ref().unwrap().score_handle,
            Some(finish.score_handle)
        );
        rpb3_root_finish_trace::assert_bundle_finish_source(
            finish,
            trace.score_arena.as_ref().unwrap(),
            &transcripts,
            "Bundle property",
        );
        match &finish.input {
            BundleFinishInput::Incomplete => assert!(oracle.body.is_none()),
            BundleFinishInput::Complete { read, write, body } => {
                assert_eq!(*read, oracle.read_flag);
                assert_eq!(*write, oracle.write_flag);
                assert_eq!(
                    transcripts[body.index()],
                    authoritative_par_score(oracle.body.as_ref().unwrap())
                );
            }
        }
    }

    fn check_if_finish_property(value: &If, complete: bool) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let decoded = protobuf_decoder::decode_if(bytes.as_slice()).expect("generated If decoder");
        let oracle = If::decode(bytes.as_slice()).expect("Prost If decoder");
        assert_eq!(decoded, oracle);
        assert_eq!(oracle.encode_to_vec(), bytes);
        let (trace, transcripts) =
            checked_property_finish(value, &bytes, &IF_RHO_MATCH_PROGRAM, complete);
        assert_eq!(trace.if_finishes.len(), 1);
        let finish = &trace.if_finishes[0];
        assert_eq!(finish.body, bytes);
        assert_eq!(finish.complete, complete);
        assert_eq!(
            trace.returned.as_ref().unwrap().score_handle,
            Some(finish.score_handle)
        );
        rpb3_root_finish_trace::assert_if_finish_source(
            finish,
            trace.score_arena.as_ref().unwrap(),
            &transcripts,
            "If property",
        );
        match &finish.input {
            IfFinishInput::Incomplete => assert!(
                oracle.condition.is_none() || oracle.if_true.is_none() || oracle.if_false.is_none()
            ),
            IfFinishInput::Complete {
                condition,
                if_true,
                if_false,
                connective_used,
            } => {
                for (handle, child) in [
                    (condition, oracle.condition.as_ref()),
                    (if_true, oracle.if_true.as_ref()),
                    (if_false, oracle.if_false.as_ref()),
                ] {
                    assert_eq!(
                        transcripts[handle.index()],
                        authoritative_par_score(child.unwrap())
                    );
                }
                assert_eq!(*connective_used, oracle.connective_used);
            }
        }
    }

    fn check_match_finish_property(value: &Match, complete: bool) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let decoded =
            protobuf_decoder::decode_match(bytes.as_slice()).expect("generated Match decoder");
        let oracle = Match::decode(bytes.as_slice()).expect("Prost Match decoder");
        assert_eq!(decoded, oracle);
        assert_eq!(oracle.encode_to_vec(), bytes);
        let (trace, transcripts) =
            checked_property_finish(value, &bytes, &MATCH_RHO_MATCH_PROGRAM, complete);
        assert_eq!(trace.match_finishes.len(), 1);
        let finish = &trace.match_finishes[0];
        assert_eq!(finish.body, bytes);
        assert_eq!(finish.complete, complete);
        assert_eq!(
            trace.returned.as_ref().unwrap().score_handle,
            Some(finish.score_handle)
        );
        rpb3_root_finish_trace::assert_match_finish_source(
            finish,
            trace.score_arena.as_ref().unwrap(),
            &transcripts,
            "Match property",
        );
        match &finish.input {
            MatchFinishInput::Incomplete => assert!(
                oracle.target.is_none()
                    || oracle
                        .cases
                        .iter()
                        .any(|case| case.pattern.is_none() || case.source.is_none())
            ),
            MatchFinishInput::Complete {
                target,
                cases,
                connective_used,
            } => {
                assert_eq!(
                    transcripts[target.index()],
                    authoritative_par_score(oracle.target.as_ref().unwrap())
                );
                assert_eq!(cases.len(), oracle.cases.len());
                for (handle, case) in cases.iter().zip(&oracle.cases) {
                    assert_eq!(
                        transcripts[handle.index()],
                        authoritative_match_case_score(case)
                    );
                }
                assert_eq!(*connective_used, oracle.connective_used);
            }
        }
        assert_eq!(trace.match_case_finishes.len(), oracle.cases.len());
        for (child, case) in trace.match_case_finishes.iter().zip(&oracle.cases) {
            let decoded = protobuf_decoder::decode_match_case(child.body.as_slice())
                .expect("nested generated MatchCase decoder");
            let nested_oracle =
                MatchCase::decode(child.body.as_slice()).expect("nested Prost MatchCase decoder");
            assert_eq!(decoded, nested_oracle);
            assert_eq!(&nested_oracle, case);
            rpb3_root_finish_trace::assert_match_case_finish_source(
                child,
                trace.score_arena.as_ref().unwrap(),
                &transcripts,
                "nested MatchCase property",
            );
            assert_eq!(
                child
                    .score_handle
                    .map(|handle| &transcripts[handle.index()]),
                authoritative_match_case_score_if_complete(case).as_ref(),
            );
        }
    }

    fn authoritative_match_case_score_if_complete(value: &MatchCase) -> Option<Vec<u8>> {
        (value.pattern.is_some() && value.source.is_some())
            .then(|| authoritative_match_case_score(value))
    }

    fn check_receive_bind_finish_property(value: &ReceiveBind) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let decoded = protobuf_decoder::decode_receive_bind(bytes.as_slice())
            .expect("generated ReceiveBind decoder");
        let oracle = ReceiveBind::decode(bytes.as_slice()).expect("Prost ReceiveBind decoder");
        assert_eq!(decoded, oracle);
        assert_eq!(oracle.encode_to_vec(), bytes);
        let (trace, transcripts) = checked_aux_finish(&bytes, &RECEIVEBIND_RHO_MATCH_PROGRAM);
        assert_eq!(trace.receive_bind_finishes.len(), 1);
        let finish = &trace.receive_bind_finishes[0];
        assert_eq!(finish.body, bytes);
        assert_eq!(
            trace.returned.as_ref().unwrap().score_handle,
            finish.score_handle
        );
        rpb3_root_finish_trace::assert_receive_bind_finish_source(
            finish,
            trace.score_arena.as_ref().unwrap(),
            &transcripts,
            "ReceiveBind property",
        );
        assert_eq!(
            finish
                .score_handle
                .map(|handle| &transcripts[handle.index()]),
            authoritative_receive_bind_score(&oracle).as_ref(),
        );
        match &finish.input {
            ReceiveBindFinishInput::SourceAbsent => assert!(oracle.source.is_none()),
            ReceiveBindFinishInput::SourcePresent {
                source,
                patterns,
                remainder,
                cost,
            } => {
                assert_eq!(patterns.len(), oracle.patterns.len());
                assert_eq!(
                    transcripts[source.index()],
                    authoritative_par_score(oracle.source.as_ref().unwrap())
                );
                for (handle, pattern) in patterns.iter().zip(&oracle.patterns) {
                    assert_eq!(
                        transcripts[handle.index()],
                        authoritative_par_score(pattern)
                    );
                }
                match (remainder, oracle.remainder.as_ref()) {
                    (Some(handle), Some(value)) => {
                        assert_eq!(transcripts[handle.index()], authoritative_var_score(value));
                    }
                    (None, None) => {}
                    _ => panic!("ReceiveBind remainder presence differs from oracle"),
                }
                match (cost, oracle.cost_region.as_ref()) {
                    (Some(handle), Some(value)) => {
                        assert_eq!(
                            transcripts[handle.index()],
                            authoritative_cost_region_score(value)
                        );
                    }
                    (None, None) => {}
                    _ => panic!("ReceiveBind cost presence differs from oracle"),
                }
            }
        }
    }

    fn check_match_case_finish_property(value: &MatchCase) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let decoded = protobuf_decoder::decode_match_case(bytes.as_slice())
            .expect("generated MatchCase decoder");
        let oracle = MatchCase::decode(bytes.as_slice()).expect("Prost MatchCase decoder");
        assert_eq!(decoded, oracle);
        assert_eq!(oracle.encode_to_vec(), bytes);
        let (trace, transcripts) = checked_aux_finish(&bytes, &MATCHCASE_RHO_MATCH_PROGRAM);
        assert_eq!(trace.match_case_finishes.len(), 1);
        let finish = &trace.match_case_finishes[0];
        assert_eq!(finish.body, bytes);
        assert_eq!(
            trace.returned.as_ref().unwrap().score_handle,
            finish.score_handle
        );
        rpb3_root_finish_trace::assert_match_case_finish_source(
            finish,
            trace.score_arena.as_ref().unwrap(),
            &transcripts,
            "MatchCase property",
        );
        assert_eq!(
            finish
                .score_handle
                .map(|handle| &transcripts[handle.index()]),
            authoritative_match_case_score_if_complete(&oracle).as_ref(),
        );
        assert_eq!(
            finish.required_present,
            oracle.pattern.is_some() && oracle.source.is_some()
        );
        assert_eq!(
            finish.canonical_fixed_point,
            oracle
                .guard
                .as_ref()
                .is_none_or(|guard| *guard != Par::default())
        );
        match &finish.input {
            MatchCaseFinishInput::RequiredAbsent => {
                assert!(oracle.pattern.is_none() || oracle.source.is_none())
            }
            MatchCaseFinishInput::RequiredPresent {
                pattern,
                source,
                guard,
                free_count,
            } => {
                assert_eq!(*free_count, oracle.free_count);
                assert_eq!(guard.is_some(), oracle.guard.is_some());
                assert_eq!(
                    transcripts[pattern.index()],
                    authoritative_par_score(oracle.pattern.as_ref().unwrap())
                );
                assert_eq!(
                    transcripts[source.index()],
                    authoritative_par_score(oracle.source.as_ref().unwrap())
                );
                if let Some(guard) = guard {
                    assert_eq!(
                        transcripts[guard.index()],
                        authoritative_par_score(oracle.guard.as_ref().unwrap())
                    );
                }
            }
        }
    }

    fn check_receive_finish_property(value: &Receive, complete: bool) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let decoded =
            protobuf_decoder::decode_receive(bytes.as_slice()).expect("generated Receive decoder");
        let oracle = Receive::decode(bytes.as_slice()).expect("Prost Receive decoder");
        assert_eq!(decoded, oracle);
        assert_eq!(oracle.encode_to_vec(), bytes);
        let (trace, transcripts) =
            checked_property_finish(value, &bytes, &RECEIVE_RHO_MATCH_PROGRAM, complete);
        assert_eq!(trace.receive_finishes.len(), 1);
        let finish = &trace.receive_finishes[0];
        assert_eq!(finish.body, bytes);
        assert_eq!(finish.complete, complete);
        assert_eq!(
            trace.returned.as_ref().unwrap().score_handle,
            Some(finish.score_handle)
        );
        rpb3_root_finish_trace::assert_receive_finish_source(
            finish,
            trace.score_arena.as_ref().unwrap(),
            &transcripts,
            "Receive property",
        );
        match &finish.input {
            ReceiveFinishInput::Incomplete => assert!(
                oracle.body.is_none() || oracle.binds.iter().any(|bind| bind.source.is_none())
            ),
            ReceiveFinishInput::Complete {
                persistent,
                peek,
                binds,
                body,
                bind_count,
                connective_used,
                condition,
                cost,
            } => {
                assert_eq!(*persistent, oracle.persistent);
                assert_eq!(*peek, oracle.peek);
                assert_eq!(*bind_count, oracle.bind_count);
                assert_eq!(*connective_used, oracle.connective_used);
                match (cost, oracle.cost_region.as_ref()) {
                    (Some(handle), Some(value)) => {
                        assert_eq!(
                            transcripts[handle.index()],
                            authoritative_cost_region_score(value)
                        );
                    }
                    (None, None) => {}
                    _ => panic!("Receive cost presence differs from oracle"),
                }
                assert_eq!(binds.len(), oracle.binds.len());
                assert_eq!(
                    transcripts[body.index()],
                    authoritative_par_score(oracle.body.as_ref().unwrap())
                );
                if let Some(condition) = condition {
                    assert_eq!(
                        transcripts[condition.index()],
                        authoritative_par_score(oracle.condition.as_ref().unwrap())
                    );
                } else {
                    assert!(oracle.condition.is_none());
                }
                for (handle, bind) in binds.iter().zip(&oracle.binds) {
                    assert_eq!(
                        transcripts[handle.index()],
                        authoritative_receive_bind_score(bind).unwrap()
                    );
                }
            }
        }
        assert_eq!(trace.receive_bind_finishes.len(), oracle.binds.len());
        for (child, bind) in trace.receive_bind_finishes.iter().zip(&oracle.binds) {
            let decoded = protobuf_decoder::decode_receive_bind(child.body.as_slice())
                .expect("nested generated ReceiveBind decoder");
            let nested_oracle = ReceiveBind::decode(child.body.as_slice())
                .expect("nested Prost ReceiveBind decoder");
            assert_eq!(decoded, nested_oracle);
            assert_eq!(&nested_oracle, bind);
            rpb3_root_finish_trace::assert_receive_bind_finish_source(
                child,
                trace.score_arena.as_ref().unwrap(),
                &transcripts,
                "nested ReceiveBind property",
            );
            assert_eq!(
                child
                    .score_handle
                    .map(|handle| &transcripts[handle.index()]),
                authoritative_receive_bind_score(bind).as_ref(),
            );
            if let ReceiveBindFinishInput::SourcePresent {
                remainder, cost, ..
            } = &child.input
            {
                if let Some(handle) = remainder {
                    assert_eq!(
                        transcripts[handle.index()],
                        authoritative_var_score(bind.remainder.as_ref().unwrap())
                    );
                } else {
                    assert!(bind.remainder.is_none());
                }
                if let Some(handle) = cost {
                    assert_eq!(
                        transcripts[handle.index()],
                        authoritative_cost_region_score(bind.cost_region.as_ref().unwrap())
                    );
                } else {
                    assert!(bind.cost_region.is_none());
                }
            }
        }
    }

    fn check_new_finish_property(value: &New, complete: bool) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let decoded =
            protobuf_decoder::decode_new(bytes.as_slice()).expect("generated New decoder");
        let oracle = New::decode(bytes.as_slice()).expect("Prost New decoder");
        assert_eq!(decoded, oracle);
        assert_eq!(oracle.encode_to_vec(), bytes);
        let (trace, transcripts) =
            checked_property_finish(value, &bytes, &NEW_RHO_MATCH_PROGRAM, complete);
        assert_eq!(trace.new_finishes.len(), 1);
        let finish = &trace.new_finishes[0];
        assert_eq!(finish.body, bytes);
        assert_eq!(finish.complete, complete);
        assert_eq!(
            trace.returned.as_ref().unwrap().score_handle,
            Some(finish.score_handle)
        );
        rpb3_root_finish_trace::assert_new_finish_source(
            finish,
            trace.score_arena.as_ref().unwrap(),
            &transcripts,
            "New property",
        );
        match &finish.input {
            NewFinishInput::Incomplete => assert!(oracle.p.is_none()),
            NewFinishInput::Complete {
                body,
                bind_count,
                uris,
                injections,
            } => {
                assert_eq!(*bind_count, oracle.bind_count);
                assert_eq!(
                    transcripts[body.index()],
                    authoritative_par_score(oracle.p.as_ref().unwrap())
                );
                assert_eq!(uris.len(), oracle.uri.len());
                for (actual, expected) in uris.iter().zip(&oracle.uri) {
                    assert_eq!(actual, expected.as_bytes());
                }
                assert_eq!(injections.len(), oracle.injections.len());
                for ((key, handle), (expected_key, value)) in
                    injections.iter().zip(&oracle.injections)
                {
                    assert_eq!(key, expected_key.as_bytes());
                    match handle {
                        Some(handle) => {
                            assert_ne!(*value, Par::default());
                            assert_eq!(transcripts[handle.index()], authoritative_par_score(value));
                        }
                        None => assert_eq!(*value, Par::default()),
                    }
                }
            }
        }
    }

    fn check_var_finish_property(value: &Var) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let decoded =
            protobuf_decoder::decode_var(bytes.as_slice()).expect("generated Var decoder");
        let oracle = Var::decode(bytes.as_slice()).expect("Prost Var decoder");
        assert_eq!(decoded, oracle);
        assert_eq!(oracle.encode_to_vec(), bytes);
        let (trace, transcripts) = checked_aux_finish(&bytes, &VAR_RHO_MATCH_PROGRAM);
        let returned = trace.returned.as_ref().unwrap();
        let handle = returned.score_handle.unwrap().index();
        assert_eq!(transcripts[handle], authoritative_var_score(&oracle));
        let entries = trace.score_arena.as_ref().unwrap();
        let (constructor, children, expected_len) = match oracle.var_instance {
            Some(VarInstance::BoundVar(level)) => {
                assert_eq!(
                    entries[0].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(level))
                );
                (Score::BOUND_VAR, vec![1, 0], 3)
            }
            Some(VarInstance::FreeVar(level)) => {
                assert_eq!(
                    entries[0].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(level))
                );
                (Score::FREE_VAR, vec![1, 0], 3)
            }
            Some(VarInstance::Wildcard(_)) => (Score::WILDCARD, vec![0], 2),
            None => {
                assert_eq!(entries.len(), 1);
                assert_eq!(
                    entries[0].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(Score::ABSENT))
                );
                return;
            }
        };
        assert_eq!(entries.len(), expected_len);
        assert_eq!(handle, expected_len - 1);
        assert_eq!(
            entries[expected_len - 2].kind,
            CanonicalScoreSourceKind::Integer(i64::from(constructor))
        );
        assert_eq!(
            entries[expected_len - 1].kind,
            CanonicalScoreSourceKind::Node(children)
        );
    }

    fn check_cost_signature_atom_finish_property(
        value: &CostSignatureAtom,
        expected_source: &[CanonicalScoreSourceEntry],
    ) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        assert_eq!(
            protobuf_decoder::decode_cost_signature_atom(bytes.as_slice())
                .expect("generated CostSignatureAtom decoder"),
            CostSignatureAtom::decode(bytes.as_slice()).expect("Prost CostSignatureAtom decoder")
        );
        let (trace, transcripts) = checked_aux_finish(&bytes, &COSTSIGNATUREATOM_RHO_MATCH_PROGRAM);
        assert_eq!(trace.score_arena.as_deref(), Some(expected_source));
        let returned = trace.returned.as_ref().unwrap();
        assert_eq!(
            returned.score_handle.map(|handle| handle.index()),
            Some(expected_source.len() - 1)
        );
        assert_eq!(
            transcripts[expected_source.len() - 1],
            authoritative_cost_signature_atom_score(value)
        );
        if let Some(Form::Bound(bound)) = value.form.as_ref() {
            assert_eq!(trace.children.len(), 1);
            let child = &trace.children[0];
            assert!(std::ptr::eq(
                child.program,
                &COSTBOUNDSIGNATURE_RHO_MATCH_PROGRAM
            ));
            assert_eq!(child.body, protobuf_encoder::encode_to_vec(bound));
            assert_eq!(child.summary.score_handle, returned.score_handle);
            rpb3_root_finish_trace::assert_summary_score_source(
                &child.summary,
                &transcripts,
                "direct bound atom child",
            );
        } else {
            assert!(trace.children.is_empty());
        }
    }

    fn check_cost_signature_finish_property(
        value: &CostSignature,
        expected_source: &[CanonicalScoreSourceEntry],
    ) {
        let bytes = protobuf_encoder::encode_to_vec(value);
        assert_eq!(
            protobuf_decoder::decode_cost_signature(bytes.as_slice())
                .expect("generated CostSignature decoder"),
            CostSignature::decode(bytes.as_slice()).expect("Prost CostSignature decoder")
        );
        let (trace, transcripts) = checked_aux_finish(&bytes, &COSTSIGNATURE_RHO_MATCH_PROGRAM);
        assert_eq!(trace.score_arena.as_deref(), Some(expected_source));
        let returned = trace.returned.as_ref().unwrap();
        assert_eq!(
            returned.score_handle.map(|handle| handle.index()),
            Some(expected_source.len() - 1)
        );
        assert_eq!(
            transcripts[expected_source.len() - 1],
            authoritative_cost_signature_score(value)
        );
        let atom_children: Vec<_> = trace
            .children
            .iter()
            .filter(|child| std::ptr::eq(child.program, &COSTSIGNATUREATOM_RHO_MATCH_PROGRAM))
            .collect();
        assert_eq!(atom_children.len(), value.atoms.len());
        for (child, atom) in atom_children.into_iter().zip(&value.atoms) {
            assert_eq!(child.body, protobuf_encoder::encode_to_vec(atom));
            rpb3_root_finish_trace::assert_summary_score_source(
                &child.summary,
                &transcripts,
                "direct signature atom child",
            );
        }
    }

    fn check_cost_region_finish_property(value: &CostRegion) -> Vec<CanonicalScoreSourceEntry> {
        let bytes = protobuf_encoder::encode_to_vec(value);
        let decoded = protobuf_decoder::decode_cost_region(bytes.as_slice())
            .expect("generated CostRegion decoder");
        let oracle = CostRegion::decode(bytes.as_slice()).expect("Prost CostRegion decoder");
        assert_eq!(decoded, oracle);
        assert_eq!(oracle.encode_to_vec(), bytes);
        let (trace, transcripts) = checked_aux_finish(&bytes, &COSTREGION_RHO_MATCH_PROGRAM);
        let returned = trace.returned.as_ref().unwrap();
        let entries = trace.score_arena.as_ref().unwrap();
        let (region_start, signer_handle) = match oracle.signer.as_ref() {
            None => (0, None),
            Some(signer) => {
                let signature_bytes = protobuf_encoder::encode_to_vec(signer);
                assert_eq!(
                    protobuf_decoder::decode_cost_signature(signature_bytes.as_slice())
                        .expect("generated CostSignature decoder"),
                    CostSignature::decode(signature_bytes.as_slice())
                        .expect("Prost CostSignature decoder")
                );
                let atom_finishes: Vec<_> = trace
                    .children
                    .iter()
                    .filter(|child| {
                        std::ptr::eq(child.program, &COSTSIGNATUREATOM_RHO_MATCH_PROGRAM)
                    })
                    .collect();
                let bound_finishes: Vec<_> = trace
                    .children
                    .iter()
                    .filter(|child| {
                        std::ptr::eq(child.program, &COSTBOUNDSIGNATURE_RHO_MATCH_PROGRAM)
                    })
                    .collect();
                assert_eq!(atom_finishes.len(), signer.atoms.len());
                assert_eq!(
                    bound_finishes.len(),
                    signer
                        .atoms
                        .iter()
                        .filter(|atom| matches!(atom.form.as_ref(), Some(Form::Bound(_))))
                        .count()
                );
                let mut bound_finishes = bound_finishes.into_iter();
                let mut next = 0;
                let mut atom_handles = Vec::with_capacity(signer.atoms.len());
                for (atom, atom_finish) in signer.atoms.iter().zip(atom_finishes) {
                    let atom_bytes = protobuf_encoder::encode_to_vec(atom);
                    assert_eq!(
                        protobuf_decoder::decode_cost_signature_atom(atom_bytes.as_slice())
                            .expect("generated CostSignatureAtom decoder"),
                        CostSignatureAtom::decode(atom_bytes.as_slice())
                            .expect("Prost CostSignatureAtom decoder")
                    );
                    assert_eq!(atom_finish.body, atom_bytes);
                    let handle = match atom.form.as_ref() {
                        Some(Form::Ground(bytes))
                        | Some(Form::Quote(bytes))
                        | Some(Form::RuntimeName(bytes)) => {
                            let constructor = match atom.form.as_ref() {
                                Some(Form::Ground(_)) => Score::COST_GROUND,
                                Some(Form::Quote(_)) => Score::COST_QUOTE,
                                Some(Form::RuntimeName(_)) => Score::COST_RUNTIME_NAME,
                                _ => unreachable!(),
                            };
                            assert_eq!(
                                entries[next].kind,
                                CanonicalScoreSourceKind::BytesOwned(bytes.clone())
                            );
                            assert_eq!(
                                entries[next + 1].kind,
                                CanonicalScoreSourceKind::Integer(i64::from(constructor))
                            );
                            assert_eq!(
                                entries[next + 2].kind,
                                CanonicalScoreSourceKind::Node(vec![next + 1, next])
                            );
                            next += 3;
                            next - 1
                        }
                        Some(Form::Bound(bound)) => {
                            let bound_finish = bound_finishes
                                .next()
                                .expect("bound atom emitted its nested child");
                            assert_eq!(bound_finish.body, protobuf_encoder::encode_to_vec(bound));
                            assert_eq!(
                                entries[next].kind,
                                CanonicalScoreSourceKind::Integer(i64::from(bound.index))
                            );
                            assert_eq!(
                                entries[next + 1].kind,
                                CanonicalScoreSourceKind::Integer(i64::from(Score::COST_BOUND))
                            );
                            assert_eq!(
                                entries[next + 2].kind,
                                CanonicalScoreSourceKind::Node(vec![next + 1, next])
                            );
                            assert_eq!(
                                bound_finish
                                    .summary
                                    .score_handle
                                    .map(|handle| handle.index()),
                                Some(next + 2)
                            );
                            rpb3_root_finish_trace::assert_summary_score_source(
                                &bound_finish.summary,
                                &transcripts,
                                "signed CostRegion bound child",
                            );
                            next += 3;
                            next - 1
                        }
                        None => {
                            assert_eq!(
                                entries[next].kind,
                                CanonicalScoreSourceKind::Integer(i64::from(
                                    Score::COST_FORM_ABSENT
                                ))
                            );
                            assert_eq!(
                                entries[next + 1].kind,
                                CanonicalScoreSourceKind::Node(vec![next])
                            );
                            next += 2;
                            next - 1
                        }
                    };
                    assert_eq!(
                        atom_finish
                            .summary
                            .score_handle
                            .map(|handle| handle.index()),
                        Some(handle)
                    );
                    assert_eq!(
                        transcripts[handle],
                        authoritative_cost_signature_atom_score(atom)
                    );
                    rpb3_root_finish_trace::assert_summary_score_source(
                        &atom_finish.summary,
                        &transcripts,
                        "signed CostRegion atom",
                    );
                    atom_handles.push(handle);
                }
                assert!(bound_finishes.next().is_none());
                let signature_finishes: Vec<_> = trace
                    .children
                    .iter()
                    .filter(|child| std::ptr::eq(child.program, &COSTSIGNATURE_RHO_MATCH_PROGRAM))
                    .collect();
                assert_eq!(signature_finishes.len(), 1);
                let signature_finish = signature_finishes[0];
                assert_eq!(signature_finish.body, signature_bytes);
                assert_eq!(
                    entries[next].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(Score::COST_SIGNATURE))
                );
                assert_eq!(
                    entries[next + 1].kind,
                    CanonicalScoreSourceKind::Node(
                        std::iter::once(next).chain(atom_handles).collect()
                    )
                );
                assert_eq!(
                    signature_finish
                        .summary
                        .score_handle
                        .map(|handle| handle.index()),
                    Some(next + 1)
                );
                assert_eq!(
                    transcripts[next + 1],
                    authoritative_cost_signature_score(signer)
                );
                rpb3_root_finish_trace::assert_summary_score_source(
                    &signature_finish.summary,
                    &transcripts,
                    "signed CostRegion signature",
                );
                (next + 2, Some(next + 1))
            }
        };
        assert_eq!(
            entries[region_start].kind,
            CanonicalScoreSourceKind::Integer(i64::from(oracle.version))
        );
        for (offset, coordinate) in [
            oracle.start_line,
            oracle.start_column,
            oracle.end_line,
            oracle.end_column,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                entries[region_start + offset + 1].kind,
                CanonicalScoreSourceKind::BytesOwned(coordinate.to_be_bytes().to_vec())
            );
        }
        let (signer_handle, ordinal_index) = match signer_handle {
            Some(handle) => (handle, region_start + 5),
            None => {
                assert_eq!(
                    entries[region_start + 5].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(Score::COST_SIGNER_ABSENT))
                );
                assert_eq!(
                    entries[region_start + 6].kind,
                    CanonicalScoreSourceKind::Node(vec![region_start + 5])
                );
                (region_start + 6, region_start + 7)
            }
        };
        assert_eq!(
            entries[ordinal_index].kind,
            CanonicalScoreSourceKind::Integer(i64::from(oracle.ordinal))
        );
        assert_eq!(
            entries[ordinal_index + 1].kind,
            CanonicalScoreSourceKind::Integer(i64::from(Score::COST_REGION))
        );
        assert_eq!(
            entries[ordinal_index + 2].kind,
            CanonicalScoreSourceKind::Node(vec![
                ordinal_index + 1,
                region_start,
                region_start + 1,
                region_start + 2,
                region_start + 3,
                region_start + 4,
                signer_handle,
                ordinal_index,
            ])
        );
        assert_eq!(entries.len(), ordinal_index + 3);
        assert_eq!(
            returned.score_handle.map(|handle| handle.index()),
            Some(ordinal_index + 2)
        );
        assert_eq!(
            transcripts[ordinal_index + 2],
            authoritative_cost_region_score(&oracle)
        );
        trace.score_arena.unwrap()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn expr_ground_structural_and_nonsemantic_finish_source_property(
            boolean in any::<bool>(),
            integer in any::<i16>(),
            other_integer in any::<i16>(),
            text in prop::collection::vec(33u8..127, 1..5),
            bytes in prop::collection::vec(any::<u8>(), 1..5),
            denominator in 1u8..128,
            scale in any::<u8>(),
            bits in any::<u64>(),
            bound in 0i32..128,
        ) {
            let text = String::from_utf8(text).unwrap();
            let first = gint(i64::from(integer));
            let second = gint(i64::from(other_integer));
            let bound_var = Var { var_instance: Some(VarInstance::BoundVar(bound)) };
            let mut instances = vec![
                None,
                Some(ExprInstance::GBool(boolean)),
                Some(ExprInstance::GInt(i64::from(integer))),
                Some(ExprInstance::GString(text.clone())),
                Some(ExprInstance::GUri(text.clone())),
                Some(ExprInstance::GByteArray(bytes.clone())),
                Some(ExprInstance::GDouble(bits)),
                Some(ExprInstance::GBigInt(bytes.clone())),
                Some(ExprInstance::GBigRat(GBigRational {
                    numerator: bytes.clone(),
                    denominator: vec![denominator],
                })),
                Some(ExprInstance::GFixedPoint(GFixedPoint {
                    unscaled: bytes,
                    scale: u32::from(scale),
                })),
                Some(ExprInstance::EVarBody(EVar { v: Some(bound_var) })),
                Some(ExprInstance::EVarBody(EVar { v: None })),
                Some(ExprInstance::ENegBody(crate::rhoapi::ENeg { p: Some(first.clone()) })),
                Some(ExprInstance::ENegBody(crate::rhoapi::ENeg { p: None })),
                Some(ExprInstance::ENotBody(crate::rhoapi::ENot { p: Some(first.clone()) })),
                Some(ExprInstance::ENotBody(crate::rhoapi::ENot { p: None })),
            ];
            macro_rules! append_binary_cases {
                ($variant:ident, $body:ident) => {
                    instances.push(Some(ExprInstance::$variant(crate::rhoapi::$body {
                        p1: Some(first.clone()), p2: Some(second.clone()),
                    })));
                    instances.push(Some(ExprInstance::$variant(crate::rhoapi::$body {
                        p1: Some(first.clone()), p2: None,
                    })));
                };
            }
            append_binary_cases!(EMultBody, EMult);
            append_binary_cases!(EDivBody, EDiv);
            append_binary_cases!(EModBody, EMod);
            append_binary_cases!(EPlusBody, EPlus);
            append_binary_cases!(EMinusBody, EMinus);
            append_binary_cases!(ELtBody, ELt);
            append_binary_cases!(ELteBody, ELte);
            append_binary_cases!(EGtBody, EGt);
            append_binary_cases!(EGteBody, EGte);
            append_binary_cases!(EEqBody, EEq);
            append_binary_cases!(ENeqBody, ENeq);
            append_binary_cases!(EAndBody, EAnd);
            append_binary_cases!(EOrBody, EOr);
            append_binary_cases!(EPercentPercentBody, EPercentPercent);
            append_binary_cases!(EPlusPlusBody, EPlusPlus);
            append_binary_cases!(EMinusMinusBody, EMinusMinus);
            instances.push(Some(ExprInstance::EMatchesBody(crate::rhoapi::EMatches {
                target: Some(first.clone()), pattern: Some(second),
            })));
            instances.push(Some(ExprInstance::EMatchesBody(crate::rhoapi::EMatches {
                target: Some(first.clone()), pattern: None,
            })));
            instances.push(Some(ExprInstance::EMethodBody(crate::rhoapi::EMethod {
                method_name: text,
                target: None,
                arguments: vec![first.clone()],
                locally_free: Vec::new(),
                connective_used: boolean,
            })));
            instances.push(Some(ExprInstance::EZipperBody(crate::rhoapi::EZipper::default())));
            assert_eq!(instances.len(), 52);
            for instance in instances {
                check_generated_expr_finish_source(&Expr { expr_instance: instance });
            }
        }

        #[test]
        fn emap_pair_identity_order_and_finish_source_property(
            seed in any::<i16>(),
            value in any::<i16>(),
            bound in 0i32..128,
            connective_used in any::<bool>(),
        ) {
            let id = |value| Some(GeneratedEMapKeyIdentity::Int(value));
            let first_key = gint(i64::from(seed));
            let second_key = gint(i64::from(seed) + 1);
            let first_value = gint(i64::from(value));
            let second_value = gint(i64::from(value) + 1);
            let mut ordered_pairs_with_identity = vec![
                (KeyValuePair { key: Some(first_key.clone()), value: Some(first_value.clone()) }, id(i64::from(seed))),
                (KeyValuePair { key: Some(second_key), value: Some(second_value.clone()) }, id(i64::from(seed) + 1)),
            ];
            sort_generated_emap_pairs(&mut ordered_pairs_with_identity);
            let (ordered_pairs, ordered_identities): (Vec<_>, Vec<_>) =
                ordered_pairs_with_identity.into_iter().unzip();
            let ordered = EMap {
                kvs: ordered_pairs.clone(),
                remainder: Some(Var { var_instance: Some(VarInstance::BoundVar(bound)) }),
                connective_used,
                ..EMap::default()
            };
            check_generated_emap_finish_source(&ordered, &ordered_identities, true, true);
            let mut reversed = ordered.clone();
            reversed.kvs.reverse();
            let mut reversed_identities = ordered_identities.clone();
            reversed_identities.reverse();
            check_generated_emap_finish_source(&reversed, &reversed_identities, true, false);

            let mut duplicates = vec![
                KeyValuePair {
                    key: Some(first_key.clone().with_locally_free(vec![1])),
                    value: Some(first_value.clone()),
                },
                KeyValuePair {
                    key: Some(first_key.clone().with_locally_free(vec![2])),
                    value: Some(second_value),
                },
            ];
            assert_eq!(
                authoritative_par_score(duplicates[0].key.as_ref().unwrap()),
                authoritative_par_score(duplicates[1].key.as_ref().unwrap()),
            );
            assert_ne!(
                protobuf_encoder::encode_to_vec(duplicates[0].key.as_ref().unwrap()),
                protobuf_encoder::encode_to_vec(duplicates[1].key.as_ref().unwrap()),
            );
            duplicates.sort_by_key(protobuf_encoder::encode_to_vec);
            let duplicate_map = EMap { kvs: duplicates.clone(), ..EMap::default() };
            check_generated_emap_finish_source(&duplicate_map, &[id(i64::from(seed)); 2], true, false);

            let composite_identity = GeneratedEMapKeyIdentity::Composite(
                i64::from(seed), i64::from(seed) + 2,
            );
            let composite_other = GeneratedEMapKeyIdentity::Composite(
                i64::from(seed), i64::from(seed) + 3,
            );
            let mut composite_distinct = vec![
                (KeyValuePair {
                    key: Some(generated_emap_key(composite_identity, &[])),
                    value: Some(first_value.clone()),
                }, Some(composite_identity)),
                (KeyValuePair {
                    key: Some(generated_emap_key(composite_other, &[])),
                    value: Some(gint(i64::from(value) + 2)),
                }, Some(composite_other)),
            ];
            sort_generated_emap_pairs(&mut composite_distinct);
            let (composite_pairs, composite_ids): (Vec<_>, Vec<_>) =
                composite_distinct.into_iter().unzip();
            check_generated_emap_finish_source(
                &EMap { kvs: composite_pairs, ..EMap::default() },
                &composite_ids, true, true,
            );
            let mut composite_duplicates = vec![
                (KeyValuePair {
                    key: Some(generated_emap_key(composite_identity, &[1])),
                    value: Some(first_value.clone()),
                }, Some(composite_identity)),
                (KeyValuePair {
                    key: Some(generated_emap_key(composite_identity, &[2])),
                    value: Some(gint(i64::from(value) + 3)),
                }, Some(composite_identity)),
            ];
            assert_eq!(
                authoritative_par_score(composite_duplicates[0].0.key.as_ref().unwrap()),
                authoritative_par_score(composite_duplicates[1].0.key.as_ref().unwrap()),
            );
            assert_eq!(
                composite_duplicates[0].0.key.as_ref().unwrap(),
                composite_duplicates[1].0.key.as_ref().unwrap(),
            );
            assert_ne!(
                protobuf_encoder::encode_to_vec(composite_duplicates[0].0.key.as_ref().unwrap()),
                protobuf_encoder::encode_to_vec(composite_duplicates[1].0.key.as_ref().unwrap()),
            );
            sort_generated_emap_pairs(&mut composite_duplicates);
            let (composite_duplicate_pairs, composite_duplicate_ids): (Vec<_>, Vec<_>) =
                composite_duplicates.into_iter().unzip();
            check_generated_emap_finish_source(
                &EMap { kvs: composite_duplicate_pairs, ..EMap::default() },
                &composite_duplicate_ids, true, false,
            );

            let nested_first = GeneratedEMapKeyIdentity::NestedMap {
                key: i64::from(seed), value: i64::from(value),
            };
            let nested_second = GeneratedEMapKeyIdentity::NestedMap {
                key: i64::from(seed), value: i64::from(value) + 1,
            };
            let nested_first_key = generated_emap_key(nested_first, &[]);
            let nested_second_key = generated_emap_key(nested_second, &[]);
            assert_eq!(
                authoritative_par_score(&nested_first_key),
                authoritative_par_score(&nested_second_key),
            );
            assert_ne!(nested_first_key, nested_second_key);
            assert_ne!(
                protobuf_encoder::encode_to_vec(&nested_first_key),
                protobuf_encoder::encode_to_vec(&nested_second_key),
            );
            let mut tied_pairs = vec![
                (KeyValuePair { key: Some(nested_first_key), value: Some(first_value.clone()) }, Some(nested_first)),
                (KeyValuePair { key: Some(nested_second_key), value: Some(gint(i64::from(value) + 4)) }, Some(nested_second)),
            ];
            sort_generated_emap_pairs(&mut tied_pairs);
            let (tied_bodies, tied_ids): (Vec<_>, Vec<_>) = tied_pairs.into_iter().unzip();
            check_generated_emap_finish_source(
                &EMap { kvs: tied_bodies.clone(), ..EMap::default() },
                &tied_ids, true, true,
            );
            let mut reversed_tied_bodies = tied_bodies;
            reversed_tied_bodies.reverse();
            let mut reversed_tied_ids = tied_ids;
            reversed_tied_ids.reverse();
            check_generated_emap_finish_source(
                &EMap { kvs: reversed_tied_bodies, ..EMap::default() },
                &reversed_tied_ids, true, false,
            );
            let mut nested_duplicates = vec![
                (KeyValuePair {
                    key: Some(generated_emap_key(nested_first, &[1])),
                    value: Some(first_value.clone()),
                }, Some(nested_first)),
                (KeyValuePair {
                    key: Some(generated_emap_key(nested_first, &[2])),
                    value: Some(gint(i64::from(value) + 5)),
                }, Some(nested_first)),
            ];
            assert_eq!(
                nested_duplicates[0].0.key.as_ref().unwrap(),
                nested_duplicates[1].0.key.as_ref().unwrap(),
            );
            sort_generated_emap_pairs(&mut nested_duplicates);
            let (nested_duplicate_pairs, nested_duplicate_ids): (Vec<_>, Vec<_>) =
                nested_duplicates.into_iter().unzip();
            check_generated_emap_finish_source(
                &EMap { kvs: nested_duplicate_pairs, ..EMap::default() },
                &nested_duplicate_ids, true, false,
            );

            let empty_identity = GeneratedEMapKeyIdentity::Empty;
            let empty_key = generated_emap_key(empty_identity, &[]);
            let empty_with_locally_free = generated_emap_key(empty_identity, &[1]);
            assert_eq!(authoritative_par_score(&empty_key), authoritative_par_score(&empty_with_locally_free));
            assert_ne!(protobuf_encoder::encode_to_vec(&empty_key), protobuf_encoder::encode_to_vec(&empty_with_locally_free));
            for key in [&empty_key, &empty_with_locally_free] {
                check_generated_emap_finish_source(
                    &EMap { kvs: vec![KeyValuePair { key: Some((*key).clone()), value: Some(first_value.clone()) }], ..EMap::default() },
                    &[Some(empty_identity)], true, true,
                );
            }
            let mut empty_duplicates = vec![
                (KeyValuePair { key: Some(empty_key), value: Some(first_value.clone()) }, Some(empty_identity)),
                (KeyValuePair { key: Some(empty_with_locally_free), value: Some(gint(i64::from(value) + 6)) }, Some(empty_identity)),
            ];
            sort_generated_emap_pairs(&mut empty_duplicates);
            let (empty_duplicate_pairs, empty_duplicate_ids): (Vec<_>, Vec<_>) =
                empty_duplicates.into_iter().unzip();
            check_generated_emap_finish_source(
                &EMap { kvs: empty_duplicate_pairs, ..EMap::default() },
                &empty_duplicate_ids, true, false,
            );

            let missing_key = KeyValuePair { key: None, value: Some(first_value) };
            let missing_value = KeyValuePair { key: Some(first_key), value: None };
            let both_missing = KeyValuePair { key: None, value: None };
            let fixtures = [
                (EMap::default(), Vec::new(), true, true),
                (EMap { kvs: vec![missing_key.clone()], ..EMap::default() }, vec![None], false, true),
                (EMap { kvs: vec![missing_value], ..EMap::default() }, vec![id(i64::from(seed))], false, true),
                (EMap { kvs: vec![both_missing], ..EMap::default() }, vec![None], false, true),
                (EMap { kvs: [ordered_pairs, vec![missing_key]].concat(), ..EMap::default() }, [ordered_identities, vec![None]].concat(), false, true),
                (EMap { kvs: [duplicates, vec![KeyValuePair::default()]].concat(), ..EMap::default() }, vec![id(i64::from(seed)), id(i64::from(seed)), None], false, false),
            ];
            for (map, identities, complete, fixed) in &fixtures {
                check_generated_emap_finish_source(map, identities, *complete, *fixed);
            }
        }

        #[test]
        fn send_finish_source_property(
            channel in any::<i16>(),
            data in prop::collection::vec(any::<i16>(), 0..5),
            persistent in any::<bool>(),
            connective_used in any::<bool>(),
        ) {
            let complete = Send {
                chan: Some(gint(i64::from(channel))),
                data: data.into_iter().map(|value| gint(i64::from(value))).collect(),
                persistent,
                connective_used,
                ..Send::default()
            };
            check_send_finish_property(&complete, true);
            let incomplete = Send { chan: None, ..complete };
            check_send_finish_property(&incomplete, false);
        }

        #[test]
        fn no_score_structural_child_pop_accept_property(
            numerator in prop::collection::vec(any::<u8>(), 1..5),
            denominator in 1u8..128,
            unscaled in prop::collection::vec(any::<u8>(), 1..5),
            scale in any::<u8>(),
            index in any::<i16>(),
            depth in any::<i16>(),
            pars in prop::collection::vec(any::<i16>(), 0..4),
            identity in prop::collection::vec(any::<u8>(), 1..8),
        ) {
            use rpb3_root_finish_trace::NoScorePayload as Payload;
            use rpb3_root_finish_trace::RootSemanticVariant as Variant;

            let rational = GBigRational { numerator, denominator: vec![denominator] };
            let expr = Expr { expr_instance: Some(ExprInstance::GBigRat(rational.clone())) };
            let bytes = generated_prost_root_bytes(&expr, |bytes| {
                protobuf_decoder::decode_expr(bytes).expect("generated rational Expr decoder")
            });
            check_no_score_child(
                &bytes, &EXPR_RHO_MATCH_PROGRAM, &rational, &GBIGRATIONAL_RHO_MATCH_PROGRAM,
                Variant::BigRational,
                Payload::BigRational {
                    numerator: rational.numerator.clone(),
                    denominator: rational.denominator.clone(),
                },
                Some(true),
            );

            let fixed = GFixedPoint { unscaled, scale: u32::from(scale) };
            let expr = Expr { expr_instance: Some(ExprInstance::GFixedPoint(fixed.clone())) };
            let bytes = generated_prost_root_bytes(&expr, |bytes| {
                protobuf_decoder::decode_expr(bytes).expect("generated fixed-point Expr decoder")
            });
            check_no_score_child(
                &bytes, &EXPR_RHO_MATCH_PROGRAM, &fixed, &GFIXEDPOINT_RHO_MATCH_PROGRAM,
                Variant::FixedPoint,
                Payload::FixedPoint {
                    unscaled: fixed.unscaled.clone(),
                    scale: fixed.scale,
                },
                Some(true),
            );

            let wildcard = crate::rhoapi::var::WildcardMsg {};
            let var = Var { var_instance: Some(VarInstance::Wildcard(wildcard.clone())) };
            let bytes = generated_prost_root_bytes(&var, |bytes| {
                protobuf_decoder::decode_var(bytes).expect("generated wildcard Var decoder")
            });
            check_no_score_child(
                &bytes, &VAR_RHO_MATCH_PROGRAM, &wildcard, &VAR_WILDCARDMSG_RHO_MATCH_PROGRAM,
                Variant::Unit, Payload::Unit, None,
            );

            let var_ref = VarRef { index: i32::from(index), depth: i32::from(depth) };
            let connective = Connective {
                connective_instance: Some(ConnectiveInstance::VarRefBody(var_ref.clone())),
            };
            let bytes = generated_prost_root_bytes(&connective, |bytes| {
                protobuf_decoder::decode_connective(bytes).expect("generated VarRef Connective decoder")
            });
            check_no_score_child(
                &bytes, &CONNECTIVE_RHO_MATCH_PROGRAM, &var_ref, &VARREF_RHO_MATCH_PROGRAM,
                Variant::VarRef,
                Payload::VarRef { index: var_ref.index, depth: var_ref.depth },
                Some(true),
            );

            let body = ConnectiveBody {
                ps: pars.into_iter().map(|value| gint(i64::from(value))).collect(),
            };
            let connective = Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(body.clone())),
            };
            let bytes = generated_prost_root_bytes(&connective, |bytes| {
                protobuf_decoder::decode_connective(bytes).expect("generated connective body decoder")
            });
            check_no_score_child(
                &bytes, &CONNECTIVE_RHO_MATCH_PROGRAM, &body, &CONNECTIVEBODY_RHO_MATCH_PROGRAM,
                Variant::ConnectiveBody,
                Payload::ConnectiveBody(body.ps.iter().map(authoritative_par_score).collect()),
                Some(true),
            );
            let connective = Connective {
                connective_instance: Some(ConnectiveInstance::ConnOrBody(body.clone())),
            };
            let bytes = generated_prost_root_bytes(&connective, |bytes| {
                protobuf_decoder::decode_connective(bytes)
                    .expect("generated disjunction body decoder")
            });
            check_no_score_child(
                &bytes, &CONNECTIVE_RHO_MATCH_PROGRAM, &body, &CONNECTIVEBODY_RHO_MATCH_PROGRAM,
                Variant::ConnectiveBody,
                Payload::ConnectiveBody(body.ps.iter().map(authoritative_par_score).collect()),
                Some(true),
            );

            let private = GPrivate { id: identity.clone() };
            let unforgeable = GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(private.clone())),
            };
            let bytes = generated_prost_root_bytes(&unforgeable, |bytes| {
                protobuf_decoder::decode_g_unforgeable(bytes).expect("generated private decoder")
            });
            check_no_score_child(
                &bytes, &GUNFORGEABLE_RHO_MATCH_PROGRAM, &private, &GPRIVATE_RHO_MATCH_PROGRAM,
                Variant::Bytes, Payload::Bytes(private.id.clone()), Some(true),
            );

            let deploy = GDeployId { sig: identity.clone() };
            let unforgeable = GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployIdBody(deploy.clone())),
            };
            let bytes = generated_prost_root_bytes(&unforgeable, |bytes| {
                protobuf_decoder::decode_g_unforgeable(bytes).expect("generated deploy ID decoder")
            });
            check_no_score_child(
                &bytes, &GUNFORGEABLE_RHO_MATCH_PROGRAM, &deploy, &GDEPLOYID_RHO_MATCH_PROGRAM,
                Variant::Bytes, Payload::Bytes(deploy.sig.clone()), Some(true),
            );

            let deployer = GDeployerId { public_key: identity };
            let unforgeable = GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployerIdBody(deployer.clone())),
            };
            let bytes = generated_prost_root_bytes(&unforgeable, |bytes| {
                protobuf_decoder::decode_g_unforgeable(bytes).expect("generated deployer ID decoder")
            });
            check_no_score_child(
                &bytes, &GUNFORGEABLE_RHO_MATCH_PROGRAM, &deployer, &GDEPLOYERID_RHO_MATCH_PROGRAM,
                Variant::Bytes, Payload::Bytes(deployer.public_key.clone()), Some(true),
            );

            let sys_auth = GSysAuthToken {};
            let unforgeable = GUnforgeable {
                unf_instance: Some(UnfInstance::GSysAuthTokenBody(sys_auth.clone())),
            };
            let bytes = generated_prost_root_bytes(&unforgeable, |bytes| {
                protobuf_decoder::decode_g_unforgeable(bytes).expect("generated system auth decoder")
            });
            check_no_score_child(
                &bytes, &GUNFORGEABLE_RHO_MATCH_PROGRAM, &sys_auth, &GSYSAUTHTOKEN_RHO_MATCH_PROGRAM,
                Variant::Unit, Payload::Unit, Some(true),
            );

            let absent_var = EVar { v: None };
            let expr = Expr { expr_instance: Some(ExprInstance::EVarBody(absent_var.clone())) };
            let bytes = generated_prost_root_bytes(&expr, |bytes| {
                protobuf_decoder::decode_expr(bytes).expect("generated absent EVar Expr decoder")
            });
            check_no_score_child(
                &bytes, &EXPR_RHO_MATCH_PROGRAM, &absent_var, &EVAR_RHO_MATCH_PROGRAM,
                Variant::ExprPayload, Payload::ExprPayloadAbsent, Some(false),
            );
        }

        #[test]
        fn no_score_pair_and_opaque_child_pop_accept_property(
            key in any::<i16>(),
            value in any::<i16>(),
            body_value in any::<i16>(),
            pattern_value in any::<i16>(),
            region_seed in any::<u16>(),
            remainder_kind in 0u8..3,
            random_state in prop::collection::vec(any::<u8>(), 0..6),
        ) {
            use rpb3_root_finish_trace::NoScorePayload as Payload;
            use rpb3_root_finish_trace::RootSemanticVariant as Variant;

            assert_direct_child_descriptor(&EMAP_RHO_MATCH_PROGRAM, &KEYVALUEPAIR_RHO_MATCH_PROGRAM, 1);
            let pair = KeyValuePair {
                key: Some(gint(i64::from(key))),
                value: Some(gint(i64::from(value))),
            };
            let emap = EMap { kvs: vec![pair.clone()], ..EMap::default() };
            let bytes = generated_prost_root_bytes(&emap, |bytes| {
                protobuf_decoder::decode_e_map(bytes).expect("generated EMap decoder")
            });
            let trace = check_no_score_child(
                &bytes, &EMAP_RHO_MATCH_PROGRAM, &pair, &KEYVALUEPAIR_RHO_MATCH_PROGRAM,
                Variant::KeyValuePair,
                Payload::KeyValuePair {
                    key: pair.key.as_ref().map(authoritative_par_score),
                    value: pair.value.as_ref().map(authoritative_par_score),
                },
                None,
            );
            assert!(trace.returned.as_ref().unwrap().score_handle.is_some());
            let incomplete_pair = KeyValuePair { value: None, ..pair };
            let emap = EMap { kvs: vec![incomplete_pair.clone()], ..EMap::default() };
            let bytes = generated_prost_root_bytes(&emap, |bytes| {
                protobuf_decoder::decode_e_map(bytes).expect("generated incomplete EMap decoder")
            });
            let trace = check_no_score_child(
                &bytes, &EMAP_RHO_MATCH_PROGRAM, &incomplete_pair, &KEYVALUEPAIR_RHO_MATCH_PROGRAM,
                Variant::KeyValuePair,
                Payload::KeyValuePair {
                    key: incomplete_pair.key.as_ref().map(authoritative_par_score),
                    value: None,
                },
                None,
            );
            assert!(trace.returned.as_ref().unwrap().score_handle.is_none());

            assert_direct_child_descriptor(
                &TAGGEDCONTINUATION_RHO_MATCH_PROGRAM, &PARWITHRANDOM_RHO_MATCH_PROGRAM, 1,
            );
            assert_direct_child_descriptor(
                &TAGGEDCONTINUATION_RHO_MATCH_PROGRAM, &COSTCLAUSEREGION_RHO_MATCH_PROGRAM, 5,
            );
            let body = gint(i64::from(body_value));
            let with_random = ParWithRandom { body: Some(body.clone()), random_state };
            let region = property_cost_region(region_seed);
            let clause = CostClauseRegion { bind_index: u32::from(region_seed), region: Some(region.clone()) };
            let tagged = TaggedContinuation {
                tagged_cont: Some(TaggedCont::ParBody(with_random.clone())),
                clause_regions: vec![clause.clone()],
                ..TaggedContinuation::default()
            };
            let bytes = generated_prost_root_bytes(&tagged, |bytes| {
                protobuf_decoder::decode_tagged_continuation(bytes).expect("generated tagged continuation decoder")
            });
            let trace = check_no_score_child(
                &bytes, &TAGGEDCONTINUATION_RHO_MATCH_PROGRAM, &with_random,
                &PARWITHRANDOM_RHO_MATCH_PROGRAM, Variant::Opaque, Payload::Opaque, None,
            );
            assert!(trace.returned.as_ref().unwrap().score_handle.is_none());
            assert_nested_child_score(
                &trace, &PAR_PROTOBUF_REGION_PROGRAM, &protobuf_encoder::encode_to_vec(&body),
                &authoritative_par_score(&body),
            );
            let trace = check_no_score_child(
                &bytes, &TAGGEDCONTINUATION_RHO_MATCH_PROGRAM, &clause,
                &COSTCLAUSEREGION_RHO_MATCH_PROGRAM, Variant::Opaque, Payload::Opaque, None,
            );
            assert_nested_child_score(
                &trace, &COSTREGION_RHO_MATCH_PROGRAM, &protobuf_encoder::encode_to_vec(&region),
                &authoritative_cost_region_score(&region),
            );

            assert_direct_child_descriptor(
                &LISTBINDPATTERNS_RHO_MATCH_PROGRAM, &BINDPATTERN_RHO_MATCH_PROGRAM, 1,
            );
            let pattern = gint(i64::from(pattern_value));
            let remainder = property_var((region_seed % 16) as i16, remainder_kind);
            let bind = BindPattern {
                patterns: vec![pattern.clone()],
                remainder: Some(remainder.clone()),
                free_count: 1,
            };
            let list = ListBindPatterns { patterns: vec![bind.clone()] };
            let bytes = generated_prost_root_bytes(&list, |bytes| {
                protobuf_decoder::decode_list_bind_patterns(bytes).expect("generated bind-pattern list decoder")
            });
            let trace = check_no_score_child(
                &bytes, &LISTBINDPATTERNS_RHO_MATCH_PROGRAM, &bind, &BINDPATTERN_RHO_MATCH_PROGRAM,
                Variant::Opaque, Payload::Opaque, None,
            );
            assert!(trace.returned.as_ref().unwrap().score_handle.is_none());
            assert_nested_child_score(
                &trace, &PAR_PROTOBUF_REGION_PROGRAM, &protobuf_encoder::encode_to_vec(&pattern),
                &authoritative_par_score(&pattern),
            );
            assert_nested_child_score(
                &trace, &VAR_RHO_MATCH_PROGRAM, &protobuf_encoder::encode_to_vec(&remainder),
                &authoritative_var_score(&remainder),
            );
        }

        #[test]
        fn epathmap_snapshot_receipt_score_source_property(
            key in any::<i16>(),
            value in any::<i16>(),
            mode in 0u8..3,
            connective_used in any::<bool>(),
        ) {
            let map = match mode {
                0 => EPathMap::default(),
                1 => EPathMap::from_set_iter(
                    [gint(i64::from(key))], Vec::new(), connective_used, None,
                ),
                _ => EPathMap::new_map(
                    [(gint(i64::from(key)), gint(i64::from(value)))],
                    Vec::new(), connective_used, None,
                ),
            };
            let expr = Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(map.clone())),
            };
            let bytes = generated_prost_root_bytes(&expr, |bytes| {
                protobuf_decoder::decode_expr(bytes).expect("generated EPathMap Expr decoder")
            });
            let child_bytes = protobuf_encoder::encode_to_vec(&map);
            let snapshot = independent_epathmap_snapshot_field(&child_bytes);
            assert_eq!(snapshot, map.trie_snapshot());
            assert!(snapshot.starts_with(b"EPM1"));
            let positions: Vec<_> = bytes.windows(snapshot.len())
                .enumerate()
                .filter_map(|(position, window)| (window == snapshot.as_slice()).then_some(position))
                .collect();
            assert_eq!(positions.len(), 1);
            let snapshot_position = positions[0];
            assert_eq!(&bytes[snapshot_position..snapshot_position + 4], b"EPM1");
            let generated = protobuf_decoder::decode_epath_map(child_bytes.as_slice())
                .expect("generated EPathMap decoder");
            let prost = EPathMap::decode(child_bytes.as_slice()).expect("Prost EPathMap decoder");
            assert_eq!(generated.encode_to_vec(), prost.encode_to_vec());
            assert_eq!(prost.encode_to_vec(), child_bytes);
            rpb3_root_finish_trace::start();
            let admission = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&bytes), &EXPR_RHO_MATCH_PROGRAM,
            ).expect("generated EPathMap Expr admission");
            let trace = rpb3_root_finish_trace::take();
            assert_eq!(trace.epathmap_accepts.len(), 1);
            let receipt = &trace.epathmap_accepts[0];
            assert_eq!(receipt.parent_type_id, EPATHMAP_RHO_MATCH_PROGRAM.type_id);
            assert_eq!((receipt.slot, receipt.tag), (3, 9));
            assert_eq!(receipt.returned_snapshot, snapshot);
            assert_eq!(receipt.stored_snapshot, snapshot);
            assert_eq!(receipt.returned_start, receipt.stored_start);
            assert_eq!(receipt.returned_start, CanonicalByteProvenance::new(0, snapshot_position));
            let pop_receipts: Vec<_> = trace.pop_accepts.iter()
                .filter(|event| event.child_kind == FrameRequestKind::EPathMap)
                .collect();
            assert_eq!(pop_receipts.len(), 1);
            let pop = pop_receipts[0];
            assert_eq!(pop.epathmap_child_snapshot.as_deref(), Some(snapshot.as_slice()));
            assert_eq!(pop.epathmap_returned_snapshot.as_deref(), Some(snapshot.as_slice()));
            assert_eq!((pop.before_len, pop.after_len), (3, Some(2)));
            assert_eq!(pop.source_len_after, Some(pop.source_len_before));
            assert_eq!(pop.parent_kind, Some(FrameRequestKind::Message));
            assert_eq!(pop.parent_before.as_ref().unwrap().program_type_id, EPATHMAP_RHO_MATCH_PROGRAM.type_id);
            assert_eq!(pop.parent_after.as_ref().unwrap().program_type_id, EPATHMAP_RHO_MATCH_PROGRAM.type_id);
            assert!(pop.parent_before.as_ref().unwrap().waiting);
            assert!(!pop.parent_after.as_ref().unwrap().waiting);

            assert_eq!(trace.epathmap_scores.len(), 1);
            let score = &trace.epathmap_scores[0];
            assert_eq!(score.body, child_bytes);
            assert_eq!(score.snapshot, snapshot);
            assert_eq!(score.source_len_after, score.source_len_before + 4);
            let entries = trace.score_arena.as_ref().unwrap();
            let before = score.source_len_before;
            let flag = i64::from(mode != 0 && connective_used);
            assert_eq!(entries[before].kind, CanonicalScoreSourceKind::BytesIndexed(snapshot.clone()));
            assert_eq!(entries[before + 1].kind, CanonicalScoreSourceKind::Integer(flag));
            assert_eq!(entries[before + 2].kind, CanonicalScoreSourceKind::Integer(i64::from(Score::EPATHMAP)));
            assert_eq!(entries[before + 3].kind, CanonicalScoreSourceKind::Node(vec![before + 2, before, before + 1]));
            assert_eq!(score.snapshot_handle.index(), before);
            assert_eq!(score.score_handle.index(), before + 3);
            let expected_score = authoritative_constructor(
                Score::EPATHMAP,
                vec![authoritative_bytes_atom(&snapshot), authoritative_integer_atom(flag)],
            );
            let transcripts = rpb3_root_finish_trace::replay_score_source(entries);
            assert_eq!(transcripts[score.score_handle.index()], expected_score);
            assert_eq!(trace.returned.as_ref().unwrap().score_handle, Some(score.score_handle));
            assert_eq!(admission.score_bytes(), Some(expected_score.as_slice()));

            let mut malformed = bytes.clone();
            malformed[snapshot_position] ^= 1;
            rpb3_root_finish_trace::start();
            assert!(validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&malformed), &EXPR_RHO_MATCH_PROGRAM,
            ).is_err());
            let refused = rpb3_root_finish_trace::take();
            assert!(refused.epathmap_accepts.is_empty());
            assert!(refused.epathmap_scores.is_empty());
        }

        #[test]
        fn bundle_finish_source_property(
            body in any::<i16>(),
            read_flag in any::<bool>(),
            write_flag in any::<bool>(),
        ) {
            let complete = Bundle {
                body: Some(gint(i64::from(body))),
                read_flag,
                write_flag,
            };
            check_bundle_finish_property(&complete, true);
            let incomplete = Bundle { body: None, ..complete };
            check_bundle_finish_property(&incomplete, false);
        }

        #[test]
        fn if_finish_source_property(
            condition in any::<i16>(),
            true_branch in any::<i16>(),
            false_branch in any::<i16>(),
            connective_used in any::<bool>(),
            missing in 0usize..3,
        ) {
            let complete = If {
                condition: Some(gint(i64::from(condition))),
                if_true: Some(gint(i64::from(true_branch))),
                if_false: Some(gint(i64::from(false_branch))),
                connective_used,
                ..If::default()
            };
            check_if_finish_property(&complete, true);
            let mut incomplete = complete;
            match missing {
                0 => incomplete.condition = None,
                1 => incomplete.if_true = None,
                _ => incomplete.if_false = None,
            }
            check_if_finish_property(&incomplete, false);
        }

        #[test]
        fn match_finish_source_property(
            target in any::<i16>(),
            case_data in prop::collection::vec(
                (any::<i16>(), any::<i16>(), 0i32..4, proptest::option::of(any::<i16>())),
                0..4,
            ),
            connective_used in any::<bool>(),
            missing_case_source in any::<bool>(),
        ) {
            let complete = Match {
                target: Some(gint(i64::from(target))),
                cases: case_data
                    .into_iter()
                    .map(|(pattern, source, free_count, guard)| MatchCase {
                        pattern: Some(gint(i64::from(pattern))),
                        source: Some(gint(i64::from(source))),
                        free_count,
                        guard: guard.map(|value| gint(i64::from(value))),
                        ..MatchCase::default()
                    })
                    .collect(),
                connective_used,
                ..Match::default()
            };
            check_match_finish_property(&complete, true);
            let mut incomplete = complete;
            if missing_case_source && !incomplete.cases.is_empty() {
                incomplete.cases[0].source = None;
            } else {
                incomplete.target = None;
            }
            check_match_finish_property(&incomplete, false);
        }

        #[test]
        fn receive_bind_finish_source_property(
            source in any::<i16>(),
            patterns in prop::collection::vec(any::<i16>(), 0..5),
            level in 0i16..16,
            remainder_kind in 0u8..3,
            region_seed in any::<u16>(),
            ground in prop::collection::vec(any::<u8>(), 1..8),
            quote in prop::collection::vec(any::<u8>(), 1..8),
            runtime_name in prop::collection::vec(any::<u8>(), 1..8),
            bound in any::<i16>(),
        ) {
            let complete = ReceiveBind {
                source: Some(gint(i64::from(source))),
                patterns: patterns.into_iter().map(|value| gint(i64::from(value))).collect(),
                ..ReceiveBind::default()
            };
            check_receive_bind_finish_property(&complete);
            let remainder = property_var(level, remainder_kind);
            let region = property_cost_region(region_seed);
            check_var_finish_property(&remainder);
            check_cost_region_finish_property(&region);
            let present = ReceiveBind {
                remainder: Some(remainder),
                cost_region: Some(region),
                ..complete
            };
            check_receive_bind_finish_property(&present);
            let atoms = property_signature_atoms(ground, quote, runtime_name, bound);
            for atom in &atoms {
                let one_atom_region = property_signed_cost_region(
                    region_seed,
                    vec![atom.clone()],
                );
                let region_source = check_cost_region_finish_property(&one_atom_region);
                let signature = one_atom_region.signer.as_ref().unwrap();
                let signature_source = &region_source[..region_source.len() - 8];
                check_cost_signature_finish_property(signature, signature_source);
                check_cost_signature_atom_finish_property(
                    atom,
                    &signature_source[..signature_source.len() - 2],
                );
            }
            let varied_len = 2 + usize::from(region_seed % 4);
            let varied_start = usize::from(region_seed % 5);
            let varied_atoms = (0..varied_len)
                .map(|offset| atoms[(varied_start + offset) % atoms.len()].clone())
                .collect();
            let varied_region = property_signed_cost_region(region_seed, varied_atoms);
            let varied_source = check_cost_region_finish_property(&varied_region);
            check_cost_signature_finish_property(
                varied_region.signer.as_ref().unwrap(),
                &varied_source[..varied_source.len() - 8],
            );
            let signed_region = property_signed_cost_region(region_seed, atoms);
            let region_source = check_cost_region_finish_property(&signed_region);
            check_cost_signature_finish_property(
                signed_region.signer.as_ref().unwrap(),
                &region_source[..region_source.len() - 8],
            );
            let signed = ReceiveBind {
                cost_region: Some(signed_region),
                ..present
            };
            check_receive_bind_finish_property(&signed);
            let incomplete = ReceiveBind { source: None, ..signed };
            check_receive_bind_finish_property(&incomplete);
        }

        #[test]
        fn match_case_finish_source_property(
            pattern in any::<i16>(),
            source in any::<i16>(),
            guard in proptest::option::of(any::<i16>()),
            free_count in 0i32..4,
            missing_source in any::<bool>(),
        ) {
            let complete = MatchCase {
                pattern: Some(gint(i64::from(pattern))),
                source: Some(gint(i64::from(source))),
                guard: guard.map(|value| gint(i64::from(value))),
                free_count,
                ..MatchCase::default()
            };
            check_match_case_finish_property(&complete);
            let mut incomplete = complete;
            if missing_source {
                incomplete.source = None;
            } else {
                incomplete.pattern = None;
            }
            check_match_case_finish_property(&incomplete);
        }

        #[test]
        fn receive_finish_source_property(
            body in any::<i16>(),
            bind_data in prop::collection::vec(
                (any::<i16>(), prop::collection::vec(any::<i16>(), 0..4)),
                0..4,
            ),
            condition in proptest::option::of(any::<i16>()),
            persistent in any::<bool>(),
            peek in any::<bool>(),
            connective_used in any::<bool>(),
            missing_bind_source in any::<bool>(),
            region_seed in any::<u16>(),
            remainder_kind in 0u8..3,
            ground in prop::collection::vec(any::<u8>(), 1..8),
            quote in prop::collection::vec(any::<u8>(), 1..8),
            runtime_name in prop::collection::vec(any::<u8>(), 1..8),
            bound in any::<i16>(),
        ) {
            let bind_count = bind_data.iter().map(|(_, patterns)| patterns.len() as i32).sum();
            let complete = Receive {
                binds: bind_data.into_iter().map(|(source, patterns)| ReceiveBind {
                    source: Some(gint(i64::from(source))),
                    patterns: patterns.into_iter().map(|value| gint(i64::from(value))).collect(),
                    ..ReceiveBind::default()
                }).collect(),
                body: Some(gint(i64::from(body))),
                condition: condition.map(|value| gint(i64::from(value))),
                persistent,
                peek,
                bind_count,
                connective_used,
                ..Receive::default()
            };
            check_receive_finish_property(&complete, true);
            let region = property_cost_region(region_seed);
            let mut present = complete;
            present.cost_region = Some(region.clone());
            for bind in &mut present.binds {
                bind.remainder = Some(property_var((region_seed % 16) as i16, remainder_kind));
                bind.cost_region = Some(region.clone());
            }
            check_receive_finish_property(&present, true);
            let atoms = property_signature_atoms(ground, quote, runtime_name, bound);
            let ground_region = property_signed_cost_region(region_seed, vec![atoms[0].clone()]);
            let mut signed_ground = present.clone();
            signed_ground.cost_region = Some(ground_region.clone());
            for bind in &mut signed_ground.binds {
                bind.cost_region = Some(ground_region.clone());
            }
            check_receive_finish_property(&signed_ground, true);
            let signed_region = property_signed_cost_region(region_seed, atoms);
            let mut signed = present;
            signed.cost_region = Some(signed_region.clone());
            for bind in &mut signed.binds {
                bind.cost_region = Some(signed_region.clone());
            }
            check_receive_finish_property(&signed, true);
            let mut incomplete = signed;
            if missing_bind_source && !incomplete.binds.is_empty() {
                incomplete.binds[0].source = None;
            } else {
                incomplete.body = None;
            }
            check_receive_finish_property(&incomplete, false);
        }

        #[test]
        fn new_collection_finish_source_property(
            body in any::<i16>(),
            value in any::<i16>(),
            bind_count in any::<i16>(),
            uri_a in any::<u8>(),
            uri_z in any::<u8>(),
            key_a in any::<u8>(),
            key_z in any::<u8>(),
        ) {
            let uri_a = format!("a{uri_a:02x}");
            let uri_z = format!("z{uri_z:02x}");
            let key_a = format!("a{key_a:02x}");
            let key_z = format!("z{key_z:02x}");
            let empty = New {
                bind_count: i32::from(bind_count),
                p: Some(gint(i64::from(body))),
                ..New::default()
            };
            check_new_finish_property(&empty, true);
            let one_uri = New { uri: vec![uri_a.clone()], ..empty.clone() };
            check_new_finish_property(&one_uri, true);
            let one_injection = New {
                injections: BTreeMap::from([(key_a.clone(), gint(i64::from(value)))]),
                ..empty.clone()
            };
            check_new_finish_property(&one_injection, true);
            let one_absent_injection = New {
                injections: BTreeMap::from([(key_a.clone(), Par::default())]),
                ..empty.clone()
            };
            check_new_finish_property(&one_absent_injection, true);
            let collections = New {
                uri: vec![uri_a, uri_z],
                injections: BTreeMap::from([
                    (key_a, gint(i64::from(value))),
                    (key_z, Par::default()),
                ]),
                ..empty
            };
            check_new_finish_property(&collections, true);
            let incomplete = New { p: None, ..collections };
            check_new_finish_property(&incomplete, false);
        }
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
    fn send_source_slot_order_rejects_decreasing_and_out_of_range_next_fields() {
        let canonical = protobuf_encoder::encode_to_vec(&Send {
            chan: Some(Par::default()),
            persistent: true,
            ..Send::default()
        });
        assert_eq!(canonical, [0x0a, 0x00, 0x18, 0x01]);
        assert!(validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&canonical),
            &SEND_RHO_MATCH_PROGRAM,
        )
        .is_ok());

        let decreasing = [0x18, 0x01, 0x0a, 0x00];
        assert!(matches!(
            validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&decreasing),
                &SEND_RHO_MATCH_PROGRAM,
            ),
            Err(CanonicalProtobufAdmissionError::Schema {
                detail: "field is outside canonical Prost slot order",
                ..
            })
        ));

        let mut out_of_range = canonical;
        out_of_range.extend_from_slice(&[0xf8, 0x07, 0x01]);
        assert!(matches!(
            validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&out_of_range),
                &SEND_RHO_MATCH_PROGRAM,
            ),
            Err(CanonicalProtobufAdmissionError::Schema {
                detail: "field is outside the generated schema",
                ..
            })
        ));
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

    fn generated_located_admission_region(
        bytes: &[u8],
        anchors: &[CanonicalByteProvenance],
        end: CanonicalByteProvenance,
    ) -> OwnedIndexedByteRegion {
        assert_eq!(bytes.len(), anchors.len());
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in bytes.iter().zip(anchors) {
            handle = arena
                .append(handle, CanonicalLocatedByte::new(byte, anchor))
                .unwrap();
        }
        OwnedIndexedByteRegion::from_frozen(arena.freeze(handle), end)
    }

    fn independent_par_varint(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
        let mut value = 0u64;
        for index in 0..10 {
            let byte = *bytes.get(*cursor)?;
            *cursor += 1;
            if index == 9 && byte > 1 {
                return None;
            }
            value |= u64::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                if index > 0 && value < (1u64 << (index * 7)) {
                    return None;
                }
                return Some(value);
            }
        }
        None
    }

    fn independent_par_root_fields(bytes: &[u8]) -> Option<Vec<(u32, usize, usize)>> {
        let mut fields = Vec::new();
        let mut cursor = 0usize;
        let mut previous = 0u32;
        let mut saw_locally_free = false;
        let mut saw_connective_used = false;
        while cursor < bytes.len() {
            let start = cursor;
            let key = independent_par_varint(bytes, &mut cursor)?;
            let tag = u32::try_from(key >> 3).ok()?;
            let wire = key & 7;
            if tag < previous {
                return None;
            }
            previous = tag;
            match tag {
                1 | 2 | 4 | 5 | 6 | 7 | 8 | 11 | 12 | 13 | 9 if wire == 2 => {
                    let len = usize::try_from(independent_par_varint(bytes, &mut cursor)?).ok()?;
                    let end = cursor.checked_add(len)?;
                    if end > bytes.len() || (tag == 9 && (saw_locally_free || len == 0)) {
                        return None;
                    }
                    if tag == 9 {
                        saw_locally_free = true;
                    }
                    cursor = end;
                }
                10 if wire == 0 => {
                    if saw_connective_used || independent_par_varint(bytes, &mut cursor)? != 1 {
                        return None;
                    }
                    saw_connective_used = true;
                }
                _ => return None,
            }
            fields.push((tag, start, cursor));
        }
        Some(fields)
    }

    fn independent_par_root_identity(bytes: &[u8]) -> Option<Vec<u8>> {
        let fields = independent_par_root_fields(bytes)?;
        let mut identity = Vec::with_capacity(bytes.len());
        for (tag, start, end) in fields {
            if tag != 9 {
                identity.extend_from_slice(&bytes[start..end]);
            }
        }
        Some(identity)
    }

    fn independent_unique_window_offset(haystack: &[u8], needle: &[u8]) -> usize {
        assert!(!needle.is_empty());
        let offsets: Vec<_> = haystack
            .windows(needle.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == needle).then_some(offset))
            .collect();
        assert_eq!(
            offsets.len(),
            1,
            "generated child body must have one byte window"
        );
        offsets[0]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn generated_discontinuous_empty_send_child_source_anchors_and_pop(
            parent_source in 1u32..100,
            terminal_source in 100u32..200,
            start_offset in 0usize..100_000,
        ) {
            let send = Send { chan: Some(Par::default()), ..Send::default() };
            let bytes = protobuf_encoder::encode_to_vec(&send);
            assert_eq!(bytes, [0x0a, 0x00]);
            let parent_start = CanonicalByteProvenance::new(parent_source, start_offset);
            let anchors = [
                parent_start,
                CanonicalByteProvenance::new(parent_source, start_offset + 1),
            ];
            let ordinary = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&bytes), &SEND_RHO_MATCH_PROGRAM,
            );
            for (case, terminal_offset) in [start_offset, start_offset + 2].into_iter().enumerate() {
                let terminal = CanonicalByteProvenance::new(terminal_source, terminal_offset);
                rpb3_root_finish_trace::start();
                let indexed = validate_canonical_protobuf_inner(
                    generated_located_admission_region(&bytes, &anchors, terminal),
                    &SEND_RHO_MATCH_PROGRAM,
                    None,
                );
                let trace = rpb3_root_finish_trace::take();
                assert_eq!(indexed, ordinary);
                assert_eq!(trace.child_eofs.len(), 1);
                let child = &trace.child_eofs[0].frame;
                assert_eq!(child.region_start, terminal);
                assert_eq!(child.region_end, terminal);
                assert_eq!(child.region_len, 0);
                let pop = trace.pop_accepts.iter().find(|event|
                    event.child_kind == FrameRequestKind::Message
                        && event.child_program.is_some_and(|program|
                            std::ptr::eq(program, &PAR_PROTOBUF_REGION_PROGRAM))
                ).expect("empty Send child must return to its parent");
                assert_eq!(pop.child_frame.as_ref().unwrap().region_start, terminal);
                assert_eq!(pop.child_frame.as_ref().unwrap().region_end, terminal);
                assert_eq!(pop.parent_before.as_ref().unwrap().region_start, parent_start);
                assert_eq!(pop.parent_after.as_ref().unwrap().region_start, parent_start);
                assert_eq!(pop.parent_before.as_ref().unwrap().region_end, terminal);
                assert_eq!(pop.parent_after.as_ref().unwrap().region_end, terminal);
                assert_ne!(parent_start.source, terminal.source);
                if case == 0 {
                    assert_eq!(parent_start.offset, terminal.offset);
                } else {
                    assert_eq!(parent_start.offset + 2, terminal.offset);
                }
            }
        }

        #[test]
        fn generated_fragmented_send_scalar_continue_uses_indexed_anchor_and_value(
            source_base in 1u32..100,
            start_offset in 0usize..100_000,
        ) {
            let send = Send {
                chan: Some(Par::default()),
                persistent: true,
                ..Send::default()
            };
            let bytes = protobuf_encoder::encode_to_vec(&send);
            assert_eq!(bytes, [0x0a, 0x00, 0x18, 0x01]);
            let anchors: Vec<_> = (0..bytes.len())
                .map(|index| CanonicalByteProvenance::new(
                    source_base + index as u32,
                    start_offset + index * 17,
                ))
                .collect();
            let terminal = CanonicalByteProvenance::new(500, start_offset + 1000);
            let ordinary = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&bytes), &SEND_RHO_MATCH_PROGRAM,
            );
            rpb3_root_finish_trace::start();
            let indexed = validate_canonical_protobuf_inner(
                generated_located_admission_region(&bytes, &anchors, terminal),
                &SEND_RHO_MATCH_PROGRAM, None,
            );
            let trace = rpb3_root_finish_trace::take();
            assert_eq!(indexed, ordinary);
            rpb3_root_finish_trace::assert_descriptor_driver_projection(
                &trace, &SEND_RHO_MATCH_PROGRAM, "fragmented Send scalar Continue",
            );
            let event = trace.message_fields.iter()
                .find(|event| event.program.type_id == SEND_RHO_MATCH_PROGRAM.type_id
                    && event.tag == 3)
                .expect("persistent Send must record its scalar field");
            assert!(matches!(
                trace.driver_steps[event.step_index].kind,
                rpb3_root_finish_trace::DriverStepKind::Continue,
            ));
            assert_eq!(event.encoded_range.start, 2);
            assert_eq!(event.indexed_field_position, anchors[2]);
            assert_eq!(event.field_position, anchors[2]);
            assert_ne!(event.field_position.source, anchors[0].source);
            assert_ne!(event.field_position.offset, anchors[0].offset + 2);
            let scalar = &event.frame_after.semantic_slots[event.slot]
                .last().expect("persistent scalar occurrence is retained").value;
            assert!(matches!(
                scalar,
                rpb3_root_finish_trace::RootOccurrenceValue::Scalar(
                    rpb3_root_finish_trace::RootScalarState::Bool(true)
                ),
            ));
        }

        #[test]
        fn generated_send_slots_flatten_in_accepted_source_order(
            data_count in 1usize..5,
            persistent in any::<bool>(),
            connective_used in any::<bool>(),
            source_base in 1u32..100,
            start_offset in 0usize..100_000,
        ) {
            let send = Send {
                chan: Some(Par::default()),
                data: vec![Par::default(); data_count],
                persistent,
                connective_used,
                ..Send::default()
            };
            let bytes = protobuf_encoder::encode_to_vec(&send);
            let anchors: Vec<_> = (0..bytes.len())
                .map(|index| CanonicalByteProvenance::new(
                    source_base + index as u32,
                    start_offset + index * 17,
                ))
                .collect();
            let terminal = CanonicalByteProvenance::new(500, start_offset + 1000);
            rpb3_root_finish_trace::start_with_message_success_stream();
            validate_canonical_protobuf_inner(
                generated_located_admission_region(&bytes, &anchors, terminal),
                &SEND_RHO_MATCH_PROGRAM,
                None,
            ).expect("fragmented generated Send is a canonical production admission");
            let trace = rpb3_root_finish_trace::take();
            let mut accepted_source_order = Vec::new();
            let slot_count = SEND_RHO_MATCH_PROGRAM.fields.len();
            for (step_index, step) in trace.driver_steps.iter().enumerate() {
                let scalar = if matches!(step.kind, rpb3_root_finish_trace::DriverStepKind::Continue)
                    && step.before_top_program.is_some_and(|program|
                        std::ptr::eq(program, &SEND_RHO_MATCH_PROGRAM))
                {
                    let field = trace.message_fields.iter().find(|field|
                        field.step_index == step_index
                            && std::ptr::eq(field.program, &SEND_RHO_MATCH_PROGRAM)
                    ).expect("Send scalar Continue has a wire-field event");
                    assert!(field.child_kind.is_none());
                    Some((field.slot, field.field_position))
                } else {
                    None
                };
                let accepted = trace.message_success.iter().find_map(|event| {
                    if let rpb3_root_finish_trace::MessageSuccessState::Accepted {
                        step_index: accepted_step,
                        program_type_id,
                        slot,
                        field_position,
                        ..
                    } = event {
                        (step_index == *accepted_step
                            && *program_type_id == SEND_RHO_MATCH_PROGRAM.type_id)
                            .then_some((*slot, *field_position))
                    } else {
                        None
                    }
                });
                assert!(!(scalar.is_some() && accepted.is_some()));
                let Some(identity) = scalar.or(accepted) else {
                    continue;
                };
                accepted_source_order.push(identity);
                assert!(accepted_source_order.iter().all(|(slot, _)| *slot < slot_count));
                assert!(accepted_source_order.windows(2).all(|pair|
                    pair[0].0 <= pair[1].0
                ));
                let root_after = step.after_message.as_ref()
                    .filter(|frame| std::ptr::eq(frame.program, &SEND_RHO_MATCH_PROGRAM))
                    .expect("Send scalar or child acceptance retains its parent frame");
                let flattened: Vec<_> = root_after.semantic_slots.iter().enumerate()
                    .flat_map(|(slot, occurrences)| occurrences.iter()
                        .map(move |occurrence| (slot, occurrence.field_position)))
                    .collect();
                assert_eq!(flattened, accepted_source_order);
            }
            assert!(accepted_source_order.len() >= data_count + 1);
        }

        #[test]
        fn generated_fragmented_scalar_continue_projects_oneof_values_and_repeated_slots(
            kind in 0u8..5,
            boolean in any::<bool>(),
            integer in any::<i64>(),
            text in prop::collection::vec(33u8..127, 1..12),
            bytes_value in prop::collection::vec(any::<u8>(), 1..12),
            source_base in 1u32..100,
            start_offset in 0usize..100_000,
        ) {
            let text = String::from_utf8(text).unwrap();
            let (instance, expected) = match kind {
                0 => (
                    ExprInstance::GBool(boolean),
                    rpb3_root_finish_trace::RootScalarState::Bool(boolean),
                ),
                1 => (
                    ExprInstance::GInt(integer),
                    rpb3_root_finish_trace::RootScalarState::I64(integer),
                ),
                2 => (
                    ExprInstance::GString(text.clone()),
                    rpb3_root_finish_trace::RootScalarState::String(text.as_bytes().to_vec()),
                ),
                3 => (
                    ExprInstance::GByteArray(bytes_value.clone()),
                    rpb3_root_finish_trace::RootScalarState::Bytes(bytes_value),
                ),
                _ => (
                    ExprInstance::GDouble(integer as u64),
                    rpb3_root_finish_trace::RootScalarState::U64(integer as u64),
                ),
            };
            let expr = Expr { expr_instance: Some(instance) };
            let expr_bytes = protobuf_encoder::encode_to_vec(&expr);
            let anchors: Vec<_> = (0..expr_bytes.len())
                .map(|index| CanonicalByteProvenance::new(
                    source_base + index as u32,
                    start_offset + index * 17,
                ))
                .collect();
            let terminal = CanonicalByteProvenance::new(500, start_offset + 1000);
            let ordinary = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&expr_bytes), &EXPR_RHO_MATCH_PROGRAM,
            );
            rpb3_root_finish_trace::start();
            let indexed = validate_canonical_protobuf_inner(
                generated_located_admission_region(&expr_bytes, &anchors, terminal),
                &EXPR_RHO_MATCH_PROGRAM, None,
            );
            let trace = rpb3_root_finish_trace::take();
            assert_eq!(indexed, ordinary);
            rpb3_root_finish_trace::assert_descriptor_driver_projection(
                &trace, &EXPR_RHO_MATCH_PROGRAM, "fragmented Expr scalar Continue",
            );
            let event = trace.message_fields.iter()
                .find(|event| event.program.type_id == EXPR_RHO_MATCH_PROGRAM.type_id
                    && matches!(trace.driver_steps[event.step_index].kind,
                        rpb3_root_finish_trace::DriverStepKind::Continue))
                .expect("selected Expr oneof scalar must Continue");
            assert_eq!(event.body_positions, anchors);
            assert_eq!(event.field_position, anchors[event.encoded_range.start]);
            assert_eq!(
                event.frame_after.semantic_slots[event.slot].last().unwrap().value,
                rpb3_root_finish_trace::RootOccurrenceValue::Scalar(expected),
            );

            let second_uri = format!("{text}x");
            let repeated = New {
                uri: vec![text.clone(), second_uri.clone()],
                p: Some(gint(1)),
                ..New::default()
            };
            let repeated_bytes = protobuf_encoder::encode_to_vec(&repeated);
            let repeated_anchors: Vec<_> = (0..repeated_bytes.len())
                .map(|index| CanonicalByteProvenance::new(
                    source_base + index as u32,
                    start_offset + index * 19,
                ))
                .collect();
            let repeated_terminal = CanonicalByteProvenance::new(501, start_offset + 2000);
            let ordinary = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&repeated_bytes), &NEW_RHO_MATCH_PROGRAM,
            );
            rpb3_root_finish_trace::start();
            let indexed = validate_canonical_protobuf_inner(
                generated_located_admission_region(
                    &repeated_bytes, &repeated_anchors, repeated_terminal,
                ),
                &NEW_RHO_MATCH_PROGRAM, None,
            );
            let trace = rpb3_root_finish_trace::take();
            assert_eq!(indexed, ordinary);
            rpb3_root_finish_trace::assert_descriptor_driver_projection(
                &trace, &NEW_RHO_MATCH_PROGRAM, "fragmented New repeated scalar Continue",
            );
            let uri_events: Vec<_> = trace.message_fields.iter()
                .filter(|event| event.program.type_id == NEW_RHO_MATCH_PROGRAM.type_id
                    && matches!(event.program.fields[event.slot].kind,
                        RhoMatchFieldKind::StringSequence))
                .collect();
            assert_eq!(uri_events.len(), 2);
            assert_eq!(uri_events[0].slot, uri_events[1].slot);
            assert!(trace.driver_steps[uri_events[1].step_index].before_message.as_ref()
                .unwrap().seen_slots.contains(&uri_events[1].slot));
            for (event, value) in uri_events.into_iter().zip([text.as_str(), second_uri.as_str()]) {
                assert_eq!(event.body_positions, repeated_anchors);
                assert_eq!(event.field_position,
                    repeated_anchors[event.encoded_range.start]);
                assert_eq!(event.frame_after.semantic_slots[event.slot].last().unwrap().value,
                    rpb3_root_finish_trace::RootOccurrenceValue::Scalar(
                        rpb3_root_finish_trace::RootScalarState::String(value.as_bytes().to_vec())
                    ));
            }
        }

        #[test]
        fn generated_discontinuous_new_map_value_source_anchors_and_pop(
            value in any::<i16>(),
            source_base in 10u32..100,
            offset_base in 0usize..10_000,
        ) {
            let value_par = gint(i64::from(value));
            let value_bytes = protobuf_encoder::encode_to_vec(&value_par);
            assert!(value_bytes.len() < 128);
            let mut entry = vec![0x0a, 0x01, b'k', 0x12, value_bytes.len() as u8];
            entry.extend_from_slice(&value_bytes);
            assert!(entry.len() < 128);
            let mut bytes = vec![0x22, entry.len() as u8];
            bytes.extend_from_slice(&entry);
            let new = New {
                injections: BTreeMap::from([("k".to_string(), value_par)]),
                ..New::default()
            };
            assert_eq!(protobuf_encoder::encode_to_vec(&new), bytes);
            let anchors: Vec<_> = (0..bytes.len()).map(|index|
                CanonicalByteProvenance::new(
                    source_base + index as u32,
                    offset_base + index * 17,
                )
            ).collect();
            let terminal = CanonicalByteProvenance::new(500, offset_base + 1000);
            let indexed = generated_located_admission_region(&bytes, &anchors, terminal);
            let entry_region = indexed.subview(CanonicalByteRange {
                start: 2, end: bytes.len(),
            }).unwrap();
            let mut frame = MapFrame::from_map_region(
                &NEW_RHO_MATCH_PROGRAM, 4, PAR_PROTOBUF_REGION_PROGRAM.type_id,
                entry_region,
            );
            let mut stats = CanonicalProtobufAdmissionStats::default();
            assert_eq!(frame.region_start, anchors[2]);
            assert!(matches!(map_step(&mut frame, &mut stats).unwrap(), FrameAction::Continue));
            assert_eq!(frame.key_position, Some(anchors[2]));
            assert_eq!(frame.key.as_ref().unwrap().position_at(0).unwrap().physical, anchors[4]);
            let request = match map_step(&mut frame, &mut stats).unwrap() {
                FrameAction::Push(request) => request,
                _ => panic!("map value must push its Par child"),
            };
            assert_eq!(frame.value_position, Some(anchors[5]));
            assert!(frame.waiting_value);
            let FrameRequest::Message { region: value_region, .. } = &request else {
                panic!("map value must be a Message child")
            };
            assert_eq!(value_region.position_at(0).unwrap().physical, anchors[7]);
            assert_eq!(value_region.end_position().physical, terminal);
            let (summary, _, _) = drive_admission(request, 0).unwrap();
            frame.accept(summary).unwrap();
            assert_eq!(frame.value_position, Some(anchors[5]));
            assert_eq!(frame.region_start, anchors[2]);
            assert!(!frame.waiting_value);
            let FrameAction::Complete(FrameSummary::Map(map_summary)) =
                map_step(&mut frame, &mut stats).unwrap() else {
                    panic!("accepted map entry must finish")
                };
            assert_eq!(map_summary.key_position, anchors[2]);
            assert_eq!(frame.region_start, anchors[2]);
            let ordinary = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&bytes), &NEW_RHO_MATCH_PROGRAM,
            );
            rpb3_root_finish_trace::start();
            let located = validate_canonical_protobuf_inner(
                indexed, &NEW_RHO_MATCH_PROGRAM, None,
            );
            let trace = rpb3_root_finish_trace::take();
            assert_eq!(located, ordinary);
            rpb3_root_finish_trace::assert_descriptor_driver_projection(
                &trace,
                &NEW_RHO_MATCH_PROGRAM,
                "generated discontinuous New map entry",
            );
            let value_pop = trace.pop_accepts.iter().find(|event|
                event.child_kind == FrameRequestKind::Message
                    && event.parent_kind == Some(FrameRequestKind::Map)
            ).expect("map value child must return");
            assert_eq!(value_pop.child_frame.as_ref().unwrap().region_start, anchors[7]);
            assert_eq!(value_pop.child_frame.as_ref().unwrap().region_end, terminal);
            let entry_pop = trace.pop_accepts.iter().find(|event|
                event.child_kind == FrameRequestKind::Map
            ).expect("map entry must return to New");
            assert_eq!(entry_pop.parent_before.as_ref().unwrap().region_start, anchors[0]);
            assert_eq!(entry_pop.parent_after.as_ref().unwrap().region_start, anchors[0]);
            assert_eq!(entry_pop.parent_before.as_ref().unwrap().region_end, terminal);
            assert_eq!(entry_pop.parent_after.as_ref().unwrap().region_end, terminal);
        }

        #[test]
        fn generated_fragmented_epathmap_receipt_and_frame_source_anchors(
            key in any::<i16>(),
            value in any::<i16>(),
            empty_value in any::<bool>(),
            source_base in 100u32..200,
        ) {
            let map_value = if empty_value {
                Par::default()
            } else {
                gint(i64::from(value))
            };
            let map = EPathMap::new_map(
                [(gint(i64::from(key)), map_value)],
                Vec::new(), false, None,
            );
            let expr = Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(map.clone())),
            };
            let bytes = protobuf_encoder::encode_to_vec(&expr);
            let child_bytes = protobuf_encoder::encode_to_vec(&map);
            let child_offsets: Vec<_> = bytes.windows(child_bytes.len())
                .enumerate()
                .filter_map(|(at, window)| (window == child_bytes.as_slice()).then_some(at))
                .collect();
            assert_eq!(child_offsets.len(), 1);
            let child_at = child_offsets[0];
            let snapshot = independent_epathmap_snapshot_field(&child_bytes);
            let snapshot_offsets: Vec<_> = bytes.windows(snapshot.len())
                .enumerate()
                .filter_map(|(at, window)| (window == snapshot.as_slice()).then_some(at))
                .collect();
            assert_eq!(snapshot_offsets.len(), 1);
            let snapshot_at = snapshot_offsets[0];
            let anchors: Vec<_> = (0..bytes.len()).map(|index|
                CanonicalByteProvenance::new(
                    source_base + (index % 17) as u32,
                    101 + index * 19,
                )
            ).collect();
            let terminal = CanonicalByteProvenance::new(700, 9000);
            let ordinary = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&bytes), &EXPR_RHO_MATCH_PROGRAM,
            );
            rpb3_root_finish_trace::start();
            let located = validate_canonical_protobuf_inner(
                generated_located_admission_region(&bytes, &anchors, terminal),
                &EXPR_RHO_MATCH_PROGRAM, None,
            );
            let trace = rpb3_root_finish_trace::take();
            assert_eq!(located, ordinary);
            rpb3_root_finish_trace::assert_descriptor_driver_projection(
                &trace,
                &EXPR_RHO_MATCH_PROGRAM,
                "fragmented EPathMap receipt",
            );
            rpb3_root_finish_trace::assert_fragmented_epathmap_event_sources(
                &trace,
                &snapshot,
                &anchors[snapshot_at..snapshot_at + snapshot.len()],
                anchors
                    .get(snapshot_at + snapshot.len())
                    .copied()
                    .unwrap_or(terminal),
            );
            assert_eq!(trace.epathmap_accepts.len(), 1);
            let receipt = &trace.epathmap_accepts[0];
            assert_eq!(receipt.returned_start, anchors[snapshot_at]);
            assert_eq!(receipt.stored_start, anchors[snapshot_at]);
            let epm_pop = trace.pop_accepts.iter().find(|event|
                event.child_kind == FrameRequestKind::EPathMap
            ).expect("EPM1 snapshot child must return");
            assert_eq!(epm_pop.epathmap_child_snapshot.as_deref(), Some(snapshot.as_slice()));
            assert_eq!(epm_pop.parent_before.as_ref().unwrap().region_start, anchors[child_at]);
            assert_eq!(epm_pop.parent_after.as_ref().unwrap().region_start, anchors[child_at]);
            assert_eq!(epm_pop.parent_before.as_ref().unwrap().region_end, terminal);
            assert_eq!(epm_pop.parent_after.as_ref().unwrap().region_end, terminal);
            let child_pop = trace.pop_accepts.iter().find(|event|
                event.child_kind == FrameRequestKind::Message
                    && event.child_program.is_some_and(|program|
                        std::ptr::eq(program, &EPATHMAP_RHO_MATCH_PROGRAM))
            ).expect("EPathMap message must return to Expr");
            assert_eq!(child_pop.child_frame.as_ref().unwrap().region_start, anchors[child_at]);
            assert_eq!(child_pop.child_frame.as_ref().unwrap().region_end, terminal);
            assert_eq!(child_pop.parent_before.as_ref().unwrap().region_start, anchors[0]);
            assert_eq!(child_pop.parent_after.as_ref().unwrap().region_start, anchors[0]);
        }

        #[test]
        fn generated_emap_interleaved_incomplete_pairs_preserve_source_order(
            seed in any::<i16>(),
            value in any::<i16>(),
        ) {
            let mut complete = vec![
                (
                    KeyValuePair {
                        key: Some(gint(i64::from(seed))),
                        value: Some(gint(i64::from(value))),
                    },
                    Some(GeneratedEMapKeyIdentity::Int(i64::from(seed))),
                ),
                (
                    KeyValuePair {
                        key: Some(gint(i64::from(seed) + 1)),
                        value: Some(gint(i64::from(value) + 1)),
                    },
                    Some(GeneratedEMapKeyIdentity::Int(i64::from(seed) + 1)),
                ),
            ];
            sort_generated_emap_pairs(&mut complete);
            for incomplete in [
                (KeyValuePair { key: None, value: Some(gint(i64::from(value) + 2)) }, None),
                (
                    KeyValuePair { key: Some(gint(i64::from(seed) + 2)), value: None },
                    Some(GeneratedEMapKeyIdentity::Int(i64::from(seed) + 2)),
                ),
            ] {
                let ordered = vec![complete[0].clone(), incomplete.clone(), complete[1].clone()];
                let (pairs, identities): (Vec<_>, Vec<_>) = ordered.into_iter().unzip();
                check_generated_emap_finish_source(
                    &EMap { kvs: pairs, ..EMap::default() }, &identities, false, true,
                );
                let reversed = vec![complete[1].clone(), incomplete.clone(), complete[0].clone()];
                let (pairs, identities): (Vec<_>, Vec<_>) = reversed.into_iter().unzip();
                check_generated_emap_finish_source(
                    &EMap { kvs: pairs, ..EMap::default() }, &identities, false, false,
                );
            }
            let same_key = gint(i64::from(seed));
            let mut duplicate_completes = vec![
                (
                    KeyValuePair { key: Some(same_key.clone()), value: Some(gint(i64::from(value))) },
                    Some(GeneratedEMapKeyIdentity::Int(i64::from(seed))),
                ),
                (
                    KeyValuePair { key: Some(same_key), value: Some(gint(i64::from(value) + 1)) },
                    Some(GeneratedEMapKeyIdentity::Int(i64::from(seed))),
                ),
            ];
            sort_generated_emap_pairs(&mut duplicate_completes);
            let interleaved = vec![
                duplicate_completes[0].clone(),
                (KeyValuePair::default(), None),
                duplicate_completes[1].clone(),
            ];
            let (pairs, identities): (Vec<_>, Vec<_>) = interleaved.into_iter().unzip();
            check_generated_emap_finish_source(
                &EMap { kvs: pairs, ..EMap::default() }, &identities, false, false,
            );
        }

        #[test]
        fn generated_emap_identity_strips_only_outer_locally_free(
            seed in any::<i16>(),
            value in any::<i16>(),
        ) {
            let base = gint(i64::from(seed));
            let outer_a = base.with_locally_free(vec![1]);
            let outer_b = base.with_locally_free(vec![2]);
            let outer_a_bytes = protobuf_encoder::encode_to_vec(&outer_a);
            let outer_b_bytes = protobuf_encoder::encode_to_vec(&outer_b);
            assert_ne!(outer_a_bytes, outer_b_bytes);
            assert_eq!(
                independent_par_root_identity(&outer_a_bytes),
                independent_par_root_identity(&outer_b_bytes),
            );
            let mut duplicates = vec![
                (KeyValuePair { key: Some(outer_a), value: Some(gint(i64::from(value))) },
                    Some(GeneratedEMapKeyIdentity::Int(i64::from(seed)))),
                (KeyValuePair { key: Some(outer_b), value: Some(gint(i64::from(value) + 1)) },
                    Some(GeneratedEMapKeyIdentity::Int(i64::from(seed)))),
            ];
            sort_generated_emap_pairs(&mut duplicates);
            let (pairs, identities): (Vec<_>, Vec<_>) = duplicates.into_iter().unzip();
            check_generated_emap_finish_source(
                &EMap { kvs: pairs, ..EMap::default() }, &identities, true, false,
            );

            let nested_key = |locally_free: u8| {
                Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::EMapBody(EMap {
                        kvs: vec![KeyValuePair {
                            key: Some(gint(i64::from(seed)).with_locally_free(vec![locally_free])),
                            value: Some(gint(i64::from(value))),
                        }],
                        ..EMap::default()
                    })),
                }])
            };
            let nested_a = nested_key(1);
            let nested_b = nested_key(2);
            let nested_a_bytes = protobuf_encoder::encode_to_vec(&nested_a);
            let nested_b_bytes = protobuf_encoder::encode_to_vec(&nested_b);
            assert_ne!(nested_a_bytes, nested_b_bytes);
            assert_ne!(
                independent_par_root_identity(&nested_a_bytes),
                independent_par_root_identity(&nested_b_bytes),
            );
            assert_eq!(authoritative_par_score(&nested_a), authoritative_par_score(&nested_b));
            let mut pairs = vec![
                KeyValuePair { key: Some(nested_a), value: Some(gint(i64::from(value) + 2)) },
                KeyValuePair { key: Some(nested_b), value: Some(gint(i64::from(value) + 3)) },
            ];
            pairs.sort_by(|left, right| {
                (
                    authoritative_par_score(left.key.as_ref().unwrap()),
                    protobuf_encoder::encode_to_vec(left),
                ).cmp(&(
                    authoritative_par_score(right.key.as_ref().unwrap()),
                    protobuf_encoder::encode_to_vec(right),
                ))
            });
            let ordered = EMap { kvs: pairs.clone(), ..EMap::default() };
            rpb3_root_finish_trace::start();
            let admitted = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&protobuf_encoder::encode_to_vec(&ordered)),
                &EMAP_RHO_MATCH_PROGRAM,
            );
            let trace = rpb3_root_finish_trace::take();
            assert!(admitted.is_ok());
            let finish = trace.emap_finishes.last().unwrap();
            assert!(finish.complete);
            assert!(finish.canonical_fixed_point);
            assert_eq!(finish.pair_scores.len(), 2);
            let reversed = EMap { kvs: pairs.into_iter().rev().collect(), ..EMap::default() };
            rpb3_root_finish_trace::start();
            validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&protobuf_encoder::encode_to_vec(&reversed)),
                &EMAP_RHO_MATCH_PROGRAM,
            ).unwrap();
            let trace = rpb3_root_finish_trace::take();
            assert!(!trace.emap_finishes.last().unwrap().canonical_fixed_point);
        }

        #[test]
        fn generated_par_canonical_field_scanner_and_identity(
            seed in any::<u8>(),
            long_len in 128usize..300,
        ) {
            let simple = vec![
                (Par::default().with_sends(vec![Send::default()]), 1),
                (Par::default().with_receives(vec![Receive::default()]), 2),
                (Par::default().with_news(vec![New::default()]), 4),
                (Par::default().with_exprs(vec![Expr { expr_instance: Some(ExprInstance::GInt(1)) }]), 5),
                (Par::default().with_matches(vec![Match::default()]), 6),
                (Par::default().with_unforgeables(vec![GUnforgeable::default()]), 7),
                (Par::default().with_connectives(vec![Connective::default()]), 8),
                (Par::default().with_bundles(vec![Bundle::default()]), 11),
                (Par::default().with_conditionals(vec![If::default()]), 12),
                (Par::default().with_cost_token_stacks(vec![CostTokenStack::default()]), 13),
            ];
            for (par, expected_tag) in simple {
                let bytes = protobuf_encoder::encode_to_vec(&par);
                let fields = independent_par_root_fields(&bytes)
                    .expect("Prost-canonical Par component must scan");
                assert_eq!(fields.iter().map(|field| field.0).collect::<Vec<_>>(), vec![expected_tag]);
                assert_eq!(fields[0].1, 0);
                assert_eq!(fields[0].2, bytes.len());
                assert_eq!(independent_par_root_identity(&bytes), Some(bytes.clone()));
                assert_eq!(Par::decode(bytes.as_slice()).unwrap(), par);
            }

            let mixed = Par::default()
                .with_sends(vec![Send::default()])
                .with_receives(vec![Receive::default()])
                .with_news(vec![New::default()])
                .with_exprs(vec![Expr { expr_instance: Some(ExprInstance::GInt(1)) }])
                .with_matches(vec![Match::default()])
                .with_unforgeables(vec![GUnforgeable::default()])
                .with_connectives(vec![Connective::default()])
                .with_locally_free(vec![seed])
                .with_connective_used(true)
                .with_bundles(vec![Bundle::default()])
                .with_conditionals(vec![If::default()])
                .with_cost_token_stacks(vec![CostTokenStack::default()]);
            let mixed_bytes = protobuf_encoder::encode_to_vec(&mixed);
            let mixed_fields = independent_par_root_fields(&mixed_bytes).unwrap();
            assert_eq!(
                mixed_fields.iter().map(|field| field.0).collect::<Vec<_>>(),
                vec![1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
            );
            let locally_free_field = mixed_fields.iter().find(|field| field.0 == 9).unwrap();
            let mut mixed_identity = mixed_bytes[..locally_free_field.1].to_vec();
            mixed_identity.extend_from_slice(&mixed_bytes[locally_free_field.2..]);
            assert_eq!(independent_par_root_identity(&mixed_bytes), Some(mixed_identity));

            let repeated = Par::default().with_exprs(vec![
                Expr { expr_instance: Some(ExprInstance::GInt(1)) },
                Expr { expr_instance: Some(ExprInstance::GInt(2)) },
            ]);
            let repeated_bytes = protobuf_encoder::encode_to_vec(&repeated);
            assert_eq!(
                independent_par_root_fields(&repeated_bytes)
                    .unwrap().iter().map(|field| field.0).collect::<Vec<_>>(),
                vec![5, 5],
            );

            let empty = protobuf_encoder::encode_to_vec(&Par::default());
            assert!(empty.is_empty());
            assert_eq!(independent_par_root_fields(&empty), Some(Vec::new()));
            assert_eq!(independent_par_root_identity(&empty), Some(Vec::new()));

            let only_locally_free = Par::default().with_locally_free(vec![seed; long_len]);
            let only_bytes = protobuf_encoder::encode_to_vec(&only_locally_free);
            assert_eq!(independent_par_root_fields(&only_bytes).unwrap()[0].0, 9);
            assert_eq!(independent_par_root_identity(&only_bytes), Some(Vec::new()));
            let only_admission = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&only_bytes), &PAR_PROTOBUF_REGION_PROGRAM,
            );
            assert!(only_admission.is_ok());

            let long_expr = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString("x".repeat(long_len))),
            }]);
            let long_bytes = protobuf_encoder::encode_to_vec(&long_expr);
            let mut cursor = 0;
            assert_eq!(independent_par_varint(&long_bytes, &mut cursor), Some(42));
            assert_eq!(independent_par_varint(&long_bytes, &mut cursor), Some((long_bytes.len() - cursor) as u64));
            assert!(cursor >= 3, "Par tag and multi-byte length must be present");
            assert_eq!(independent_par_root_identity(&long_bytes), Some(long_bytes.clone()));
            assert!(validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&long_bytes), &PAR_PROTOBUF_REGION_PROGRAM,
            ).is_ok());

            for malformed in [
                vec![0x4a, 0x01, 0x01, 0x4a, 0x01, 0x02],
                vec![0x8a, 0x00, 0x00],
                vec![0x0a, 0x80, 0x00],
                vec![0x1a, 0x00],
            ] {
                assert!(independent_par_root_fields(&malformed).is_none());
                assert!(validate_canonical_protobuf(
                    ProtobufRegion::copy_from_slice(&malformed), &PAR_PROTOBUF_REGION_PROGRAM,
                ).is_err());
            }
        }

        #[test]
        fn generated_discontinuous_emap_key_trace_preserves_physical_anchors(
            seed in any::<i16>(),
            source_base in 100u32..200,
        ) {
            let key = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString(format!("indexed-key-{seed}"))),
            }]);
            let value = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString(format!("indexed-value-{seed}"))),
            }]);
            let pair = KeyValuePair { key: Some(key.clone()), value: Some(value) };
            let map = EMap { kvs: vec![pair.clone()], ..EMap::default() };
            let expr = Expr { expr_instance: Some(ExprInstance::EMapBody(map.clone())) };
            let bytes = protobuf_encoder::encode_to_vec(&expr);
            let map_bytes = protobuf_encoder::encode_to_vec(&map);
            let pair_bytes = protobuf_encoder::encode_to_vec(&pair);
            let key_bytes = protobuf_encoder::encode_to_vec(&key);
            let map_at = independent_unique_window_offset(&bytes, &map_bytes);
            let pair_at = independent_unique_window_offset(&bytes, &pair_bytes);
            let key_at = independent_unique_window_offset(&bytes, &key_bytes);
            assert!(map_at < pair_at && pair_at < key_at);
            let anchors: Vec<_> = (0..bytes.len()).map(|index|
                CanonicalByteProvenance::new(source_base + (index % 23) as u32, 1000 + index * 37),
            ).collect();
            let terminal = CanonicalByteProvenance::new(source_base + 1000, 900_000);
            let anchor_at = |index: usize| {
                assert!(index <= bytes.len());
                anchors.get(index).copied().unwrap_or(terminal)
            };

            rpb3_root_finish_trace::start();
            let ordinary = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&bytes), &EXPR_RHO_MATCH_PROGRAM,
            ).expect("contiguous generated EMap Expr admission");
            let ordinary_trace = rpb3_root_finish_trace::take();
            rpb3_root_finish_trace::start();
            let located = validate_canonical_protobuf_inner(
                generated_located_admission_region(&bytes, &anchors, terminal),
                &EXPR_RHO_MATCH_PROGRAM, None,
            ).expect("discontinuous generated EMap Expr admission");
            let trace = rpb3_root_finish_trace::take();
            assert_eq!(located, ordinary);
            let ordinary_score = rpb3_root_finish_trace::replay_score_source(
                ordinary_trace.score_arena.as_ref().unwrap(),
            );
            let indexed_score = rpb3_root_finish_trace::replay_score_source(
                trace.score_arena.as_ref().unwrap(),
            );
            assert_eq!(indexed_score, ordinary_score);

            let key_eof = trace.child_eofs.iter().find(|event|
                std::ptr::eq(event.program, &PAR_PROTOBUF_REGION_PROGRAM)
                    && event.body == key_bytes
            ).expect("indexed EMap key Par must reach EOF");
            assert_eq!(key_eof.frame.region_start, anchors[key_at]);
            assert_eq!(key_eof.frame.region_end, anchor_at(key_at + key_bytes.len()));
            assert_eq!(key_eof.frame.region_len, key_bytes.len());
            let key_pop = trace.pop_accepts.iter().find(|event|
                event.child_program.is_some_and(|program|
                    std::ptr::eq(program, &PAR_PROTOBUF_REGION_PROGRAM))
                    && event.child_body.as_deref() == Some(key_bytes.as_slice())
            ).expect("indexed key Par must return to its pair");
            assert_eq!(key_pop.child_frame.as_ref().unwrap().region_start, anchors[key_at]);
            assert_eq!(key_pop.parent_before.as_ref().unwrap().region_start, anchors[pair_at]);
            assert_eq!(key_pop.parent_after.as_ref().unwrap().region_start, anchors[pair_at]);
            assert_eq!(key_pop.parent_before.as_ref().unwrap().region_end,
                anchor_at(pair_at + pair_bytes.len()));
            assert_eq!(key_pop.parent_after.as_ref().unwrap().region_end,
                anchor_at(pair_at + pair_bytes.len()));

            let pair_pop = trace.pop_accepts.iter().find(|event|
                event.child_program.is_some_and(|program|
                    std::ptr::eq(program, &KEYVALUEPAIR_RHO_MATCH_PROGRAM))
                    && event.child_body.as_deref() == Some(pair_bytes.as_slice())
            ).expect("KeyValuePair must return to its EMap parent");
            assert_eq!(pair_pop.child_frame.as_ref().unwrap().region_start, anchors[pair_at]);
            assert_eq!(pair_pop.child_frame.as_ref().unwrap().region_end,
                anchor_at(pair_at + pair_bytes.len()));
            assert_eq!(pair_pop.parent_before.as_ref().unwrap().region_start, anchors[map_at]);
            assert_eq!(pair_pop.parent_after.as_ref().unwrap().region_start, anchors[map_at]);
            assert_eq!(pair_pop.source_len_after, Some(pair_pop.source_len_before));
            assert_eq!(pair_pop.summary.as_ref().unwrap().score_handle, None);

            let finish = trace.emap_finishes.iter().find(|event|
                event.body == map_bytes
            ).expect("EMap finish must retain source pair");
            assert!(finish.complete);
            assert!(finish.canonical_fixed_point);
            assert_eq!(finish.pair_scores.len(), 1);
            assert_eq!(indexed_score[finish.pair_scores[0].index()], authoritative_par_score(&key));
            let map_pop = trace.pop_accepts.iter().find(|event|
                event.child_program.is_some_and(|program|
                    std::ptr::eq(program, &EMAP_RHO_MATCH_PROGRAM))
                    && event.child_body.as_deref() == Some(map_bytes.as_slice())
            ).expect("EMap must return to Expr tag 22");
            assert_eq!(map_pop.child_frame.as_ref().unwrap().region_start, anchors[map_at]);
            assert_eq!(map_pop.child_frame.as_ref().unwrap().region_end, terminal);
            assert_eq!(map_pop.parent_before.as_ref().unwrap().region_start, anchors[0]);
            assert_eq!(map_pop.parent_after.as_ref().unwrap().region_start, anchors[0]);
            assert_eq!(map_pop.parent_before.as_ref().unwrap().region_end, terminal);
            assert_eq!(map_pop.parent_after.as_ref().unwrap().region_end, terminal);
            let root_expr_finish = trace.expr_finishes.iter().find(|event|
                event.body == bytes
            ).expect("root EMap Expr must finish");
            assert!(root_expr_finish.complete);
        }
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

            let wrong_family = if T::TAG == ParComponentTag::Send {
                ParComponentTag::Receive
            } else {
                ParComponentTag::Send
            };
            assert!(matches!(
                validate_canonical_component_protobuf(
                    ProtobufRegion::copy_from_slice(body),
                    program,
                    wrong_family,
                    &key[score_start..score_end],
                ),
                Err(CanonicalProtobufAdmissionError::Schema { .. })
            ));

            let score = &key[score_start..score_end];
            assert!(!score.is_empty());
            let prefix_len = score.len() - 1;
            assert!(matches!(
                validate_canonical_component_protobuf(
                    ProtobufRegion::copy_from_slice(body),
                    program,
                    T::TAG,
                    &score[..prefix_len],
                ),
                Err(CanonicalProtobufAdmissionError::ScoreMismatch {
                    first_difference,
                    ..
                }) if first_difference == prefix_len
            ));
            let mut extended = score.to_vec();
            extended.push(0);
            assert!(matches!(
                validate_canonical_component_protobuf(
                    ProtobufRegion::copy_from_slice(body),
                    program,
                    T::TAG,
                    &extended,
                ),
                Err(CanonicalProtobufAdmissionError::ScoreMismatch {
                    first_difference,
                    ..
                }) if first_difference == score.len()
            ));

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

    fn assert_nested_component_finish<T: ParBagComponent>(
        value: &T,
        child: &rpb3_root_finish_trace::ChildFinishState,
        label: &str,
    ) {
        let component = child
            .summary
            .component
            .as_ref()
            .unwrap_or_else(|| panic!("{label}: child reducer returned no component"));
        assert_eq!(component.family, T::TAG, "{label}");
        assert_eq!(
            component.complete,
            T::semantic_shape_is_complete(value),
            "{label}"
        );
        assert_eq!(
            component.canonical_path_stable,
            T::canonical_path_stable(value),
            "{label}"
        );
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
            assert_eq!(&key[score_end..trailer], child.body, "{label}");
            assert_eq!(
                child.summary.score_bytes.as_deref(),
                Some(&key[score_start..score_end]),
                "{label}"
            );
        });
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
    fn every_wire_type_id_resolves_to_its_expected_semantic_kind() {
        use SemanticKind::*;

        let expected = [
            ("Par", RawPar),
            ("TaggedContinuation", Opaque),
            ("ParWithRandom", Opaque),
            ("PCost", Opaque),
            ("ListParWithRandom", Opaque),
            ("Var", Var),
            ("WildcardMsg", Unit),
            ("Bundle", Component(ParComponentTag::Bundle)),
            ("CostSignature", CostSignature),
            ("CostSignatureAtom", CostSignatureAtom),
            ("CostBoundSignature", CostBoundSignature),
            ("CostRegion", CostRegion),
            ("CostTokenStack", Component(ParComponentTag::CostTokenStack)),
            ("CostClauseRegion", Opaque),
            ("Send", Component(ParComponentTag::Send)),
            ("ReceiveBind", ReceiveBind),
            ("BindPattern", Opaque),
            ("ListBindPatterns", Opaque),
            ("Receive", Component(ParComponentTag::Receive)),
            ("New", Component(ParComponentTag::New)),
            ("MatchCase", MatchCase),
            ("Match", Component(ParComponentTag::Match)),
            ("If", Component(ParComponentTag::If)),
            ("Expr", Expr),
            ("GBigRational", BigRational),
            ("GFixedPoint", FixedPoint),
            ("EList", OrderedCollection {
                constructor: Score::ELIST,
                has_remainder: true,
                canonical_set: false,
            }),
            ("ETuple", OrderedCollection {
                constructor: Score::ETUPLE,
                has_remainder: false,
                canonical_set: false,
            }),
            ("ESet", OrderedCollection {
                constructor: Score::ESET,
                has_remainder: true,
                canonical_set: true,
            }),
            ("EMap", EMap),
            ("EPathMap", EPathMap),
            ("EZipper", EZipper),
            ("EMethod", EMethod),
            ("KeyValuePair", KeyValuePair),
            ("EVar", ExprVar),
            ("ENot", ExprUnary(Score::ENOT)),
            ("ENeg", ExprUnary(Score::ENEG)),
            ("EMult", ExprBinary(Score::EMULT)),
            ("EDiv", ExprBinary(Score::EDIV)),
            ("EMod", ExprBinary(Score::EMOD)),
            ("EPlus", ExprBinary(Score::EPLUS)),
            ("EMinus", ExprBinary(Score::EMINUS)),
            ("ELt", ExprBinary(Score::ELT)),
            ("ELte", ExprBinary(Score::ELTE)),
            ("EGt", ExprBinary(Score::EGT)),
            ("EGte", ExprBinary(Score::EGTE)),
            ("EEq", ExprBinary(Score::EEQ)),
            ("ENeq", ExprBinary(Score::ENEQ)),
            ("EAnd", ExprBinary(Score::EAND)),
            ("EOr", ExprBinary(Score::EOR)),
            ("EMatches", ExprBinary(Score::EMATCHES)),
            ("EPercentPercent", ExprBinary(Score::EPERCENT)),
            ("EPlusPlus", ExprBinary(Score::EPLUSPLUS)),
            ("EMinusMinus", ExprBinary(Score::EMINUSMINUS)),
            ("Connective", Connective),
            ("VarRef", VarRef),
            ("ConnectiveBody", ConnectiveBody),
            ("DeployId", Opaque),
            ("DeployerId", Opaque),
            ("GUnforgeable", GUnforgeable),
            ("GPrivate", BytesLeaf),
            ("GDeployId", BytesLeaf),
            ("GDeployerId", BytesLeaf),
            ("GSysAuthToken", Unit),
        ];
        assert_eq!(expected.len(), 64);
        assert_eq!(RHO_MATCH_REGISTRY.len(), expected.len());
        for (index, ((name, kind), (registered_name, registered))) in
            expected.iter().zip(RHO_MATCH_REGISTRY).enumerate()
        {
            let type_id = u16::try_from(index + 1).unwrap();
            assert_eq!(name, registered_name, "wire type ID {type_id}");
            assert_eq!(registered.type_id, type_id, "{name}");
            let resolved = wire_program_by_type_id(type_id).unwrap();
            if *name == "Par" {
                assert!(std::ptr::eq(resolved, &PAR_PROTOBUF_REGION_PROGRAM));
            } else {
                assert!(std::ptr::eq(resolved, *registered), "{name}");
            }
            assert_eq!(semantic_kind(resolved), Some(*kind), "{name}");
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
    fn generated_root_finish_trace_matches_pre_pop_and_returned_score() {
        macro_rules! check_child_component {
            ($child:ident, $label:ident, $decoder:ident) => {{
                let value = protobuf_decoder::$decoder($child.body.as_slice())
                    .unwrap_or_else(|error| panic!("{}: component decode failed: {error}", $label));
                assert_nested_component_finish(&value, $child, &$label);
            }};
        }

        let mut checked = 0usize;
        let mut checked_children = 0usize;
        let mut checked_nested_pars = 0usize;
        let mut checked_nested_epathmaps = 0usize;
        let mut checked_driver_fields = 0usize;
        let mut checked_driver_pops = 0usize;
        let mut checked_complete_sends = 0usize;
        let mut checked_incomplete_sends = 0usize;
        let mut checked_complete_bundles = 0usize;
        let mut checked_incomplete_bundles = 0usize;
        let mut checked_complete_ifs = 0usize;
        let mut checked_incomplete_ifs = 0usize;
        let mut checked_complete_matches = 0usize;
        let mut checked_incomplete_matches = 0usize;
        let mut component_counts = BTreeMap::new();
        let fixtures = schema_fixtures();
        for fixture in &fixtures {
            let label = format!("{}.{}", fixture.message_name, fixture.fixture_name);
            let machine = (fixture.machine_round_trip)(&fixture.bytes)
                .unwrap_or_else(|error| panic!("{label}: generated PDA rejected fixture: {error}"));
            let oracle = (fixture.oracle_round_trip)(&fixture.bytes).unwrap_or_else(|error| {
                panic!("{label}: recursive Prost rejected fixture: {error}")
            });
            assert_eq!(
                machine, oracle,
                "{label}: generated PDA/oracle disagreement"
            );
            assert_eq!(
                machine, fixture.bytes,
                "{label}: fixture is not a fixed point"
            );

            rpb3_root_finish_trace::start();
            let admission = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(&fixture.bytes),
                fixture.program,
            )
            .unwrap_or_else(|error| panic!("{label}: admission failed: {error}"));
            let trace = rpb3_root_finish_trace::take();
            let (fields, pops) = rpb3_root_finish_trace::assert_descriptor_driver_projection(
                &trace,
                fixture.program,
                &label,
            );
            checked_driver_fields += fields;
            checked_driver_pops += pops;
            let source_entries = trace
                .score_arena
                .as_ref()
                .expect("root return captured its score arena");
            let transcripts = rpb3_root_finish_trace::replay_score_source(source_entries);
            let eof = trace.eof.expect("root message emitted an EOF observation");
            let reduced = trace.reduced.expect("root reducer returned a summary");
            let complete = trace.complete.expect("root emitted FrameAction::Complete");
            let pre_pop = trace
                .pre_pop
                .expect("driver observed the pre-pop root summary");
            let returned = trace.returned.expect("driver returned the root summary");
            assert_eq!(eof.program_type_id, fixture.program.type_id, "{label}");
            assert_eq!(eof.region_len, fixture.bytes.len(), "{label}");
            assert_eq!(
                eof.region_start,
                CanonicalByteProvenance::new(0, 0),
                "{label}"
            );
            assert_eq!(
                eof.region_end,
                CanonicalByteProvenance::new(0, fixture.bytes.len()),
                "{label}"
            );
            assert!(eof.cursor_empty && !eof.waiting, "{label}");
            assert_eq!(eof.cursor_bytes_read, fixture.bytes.len(), "{label}");
            assert!(
                eof.cursor_bytes_traversed <= eof.cursor_bytes_read,
                "{label}"
            );
            assert_eq!(trace.reduced_frame, Some(eof.clone()), "{label}");
            assert_eq!(trace.complete_frame, Some(eof.clone()), "{label}");
            assert_eq!(trace.pre_pop_frame, Some(eof), "{label}");
            assert_eq!(reduced, complete, "{label}");
            assert_eq!(complete, pre_pop, "{label}");
            assert_eq!(pre_pop, returned, "{label}");
            assert_eq!(trace.remaining_frames, Some(0), "{label}");
            for (stage, summary) in [
                ("reduced", &reduced),
                ("complete", &complete),
                ("pre-pop", &pre_pop),
                ("returned", &returned),
            ] {
                rpb3_root_finish_trace::assert_summary_score_source(
                    summary,
                    &transcripts,
                    &format!("{label}: {stage}"),
                );
            }
            if let Some(bytes) = returned.score_bytes.as_ref() {
                assert_eq!(returned.score_encoded_len, Some(bytes.len()), "{label}");
                assert!(returned.score_handle.is_some(), "{label}");
            }

            let expected_family = match fixture.message_name {
                "Send" => Some(ParComponentTag::Send),
                "Receive" => Some(ParComponentTag::Receive),
                "New" => Some(ParComponentTag::New),
                "Expr" => Some(ParComponentTag::Expr),
                "Match" => Some(ParComponentTag::Match),
                "GUnforgeable" => Some(ParComponentTag::GUnforgeable),
                "Connective" => Some(ParComponentTag::Connective),
                "Bundle" => Some(ParComponentTag::Bundle),
                "If" => Some(ParComponentTag::If),
                "CostTokenStack" => Some(ParComponentTag::CostTokenStack),
                _ => None,
            };
            assert_eq!(
                returned
                    .component
                    .as_ref()
                    .map(|component| component.family),
                expected_family,
                "{label}"
            );
            assert_eq!(
                returned
                    .component
                    .as_ref()
                    .map(|component| CanonicalComponentAdmission {
                        family: component.family,
                        complete: component.complete,
                        canonical_path_stable: component.canonical_path_stable,
                    }),
                admission.component(),
                "{label}"
            );
            if let Some(family) = expected_family {
                assert_eq!(
                    returned.score_bytes.as_deref(),
                    admission.score_bytes(),
                    "{label}"
                );
                *component_counts.entry(family.byte()).or_insert(0usize) += 1;
            }
            for child in &trace.children {
                let child_label = format!("{label}: nested {}", child.program.type_name);
                rpb3_root_finish_trace::assert_summary_score_source(
                    &child.summary,
                    &transcripts,
                    &child_label,
                );
                assert_eq!(child.frame.region_len, child.body.len(), "{child_label}");
                assert!(
                    child.frame.cursor_empty && !child.frame.waiting,
                    "{child_label}"
                );
                assert_eq!(
                    child.frame.cursor_bytes_read,
                    child.body.len(),
                    "{child_label}"
                );
                if let Some(bytes) = child.summary.score_bytes.as_ref() {
                    assert_eq!(
                        child.summary.score_encoded_len,
                        Some(bytes.len()),
                        "{child_label}"
                    );
                }
                if std::ptr::eq(child.program, &PAR_PROTOBUF_REGION_PROGRAM) {
                    checked_nested_pars += 1;
                    let value = protobuf_decoder::decode_par(child.body.as_slice()).unwrap_or_else(
                        |error| panic!("{child_label}: Par decode failed: {error}"),
                    );
                    let oracle = Par::decode(child.body.as_slice()).unwrap_or_else(|error| {
                        panic!("{child_label}: Par oracle decode failed: {error}")
                    });
                    assert_eq!(oracle.encode_to_vec(), child.body, "{child_label}");
                    assert_eq!(
                        child.summary.score_bytes.as_deref(),
                        Some(authoritative_par_score(&value).as_slice()),
                        "{child_label}"
                    );
                } else if std::ptr::eq(child.program, &EPATHMAP_RHO_MATCH_PROGRAM) {
                    checked_nested_epathmaps += 1;
                    let machine = protobuf_decoder::decode_epath_map(child.body.as_slice())
                        .unwrap_or_else(|error| {
                            panic!("{child_label}: EPathMap machine decode failed: {error}")
                        });
                    let oracle = EPathMap::decode(child.body.as_slice()).unwrap_or_else(|error| {
                        panic!("{child_label}: EPathMap oracle decode failed: {error}")
                    });
                    assert_eq!(machine.encode_to_vec(), child.body, "{child_label}");
                    assert_eq!(oracle.encode_to_vec(), child.body, "{child_label}");
                } else {
                    let adapter = fixtures
                        .iter()
                        .find(|candidate| std::ptr::eq(candidate.program, child.program))
                        .unwrap_or_else(|| {
                            panic!("{child_label}: missing generated oracle adapter")
                        });
                    let machine =
                        (adapter.machine_round_trip)(&child.body).unwrap_or_else(|error| {
                            panic!("{child_label}: machine decode failed: {error}")
                        });
                    let oracle = (adapter.oracle_round_trip)(&child.body).unwrap_or_else(|error| {
                        panic!("{child_label}: oracle decode failed: {error}")
                    });
                    assert_eq!(machine, child.body, "{child_label}");
                    assert_eq!(oracle, child.body, "{child_label}");
                    match adapter.message_name {
                        "Send" => check_child_component!(child, child_label, decode_send),
                        "Receive" => check_child_component!(child, child_label, decode_receive),
                        "New" => check_child_component!(child, child_label, decode_new),
                        "Expr" => check_child_component!(child, child_label, decode_expr),
                        "Match" => check_child_component!(child, child_label, decode_match),
                        "GUnforgeable" => {
                            check_child_component!(child, child_label, decode_g_unforgeable)
                        }
                        "Connective" => {
                            check_child_component!(child, child_label, decode_connective)
                        }
                        "Bundle" => check_child_component!(child, child_label, decode_bundle),
                        "If" => check_child_component!(child, child_label, decode_if),
                        "CostTokenStack" => {
                            check_child_component!(child, child_label, decode_cost_token_stack)
                        }
                        _ => {}
                    }
                }
                checked_children += 1;
            }
            for finish in &trace.send_finishes {
                let body = &finish.body;
                let input = &finish.input;
                rpb3_root_finish_trace::assert_send_finish_source(
                    finish,
                    source_entries,
                    &transcripts,
                    &label,
                );
                let send = protobuf_decoder::decode_send(body.as_slice())
                    .unwrap_or_else(|error| panic!("{label}: Send decode failed: {error}"));
                assert_eq!(protobuf_encoder::encode_to_vec(&send), *body, "{label}");
                match input {
                    SendFinishInput::Incomplete => {
                        assert!(send.chan.is_none(), "{label}");
                        checked_incomplete_sends += 1;
                    }
                    SendFinishInput::Complete {
                        persistent,
                        channel,
                        data,
                        connective_used,
                        cost,
                    } => {
                        let channel_value = send
                            .chan
                            .as_ref()
                            .unwrap_or_else(|| panic!("{label}: complete Send has no channel"));
                        assert_eq!(*persistent, send.persistent, "{label}");
                        assert_eq!(*connective_used, send.connective_used, "{label}");
                        assert_eq!(cost.is_some(), send.cost_region.is_some(), "{label}");
                        assert_eq!(data.len(), send.data.len(), "{label}");
                        assert_eq!(
                            transcripts[channel.index()],
                            authoritative_par_score(channel_value),
                            "{label}"
                        );
                        for (handle, value) in data.iter().zip(&send.data) {
                            assert_eq!(
                                transcripts[handle.index()],
                                authoritative_par_score(value),
                                "{label}"
                            );
                        }
                        checked_complete_sends += 1;
                    }
                }
            }
            for finish in &trace.bundle_finishes {
                let body = &finish.body;
                rpb3_root_finish_trace::assert_bundle_finish_source(
                    finish,
                    source_entries,
                    &transcripts,
                    &label,
                );
                let bundle = protobuf_decoder::decode_bundle(body.as_slice())
                    .unwrap_or_else(|error| panic!("{label}: Bundle decode failed: {error}"));
                let oracle = Bundle::decode(body.as_slice()).unwrap_or_else(|error| {
                    panic!("{label}: Bundle oracle decode failed: {error}")
                });
                assert_eq!(bundle, oracle, "{label}");
                assert_eq!(oracle.encode_to_vec(), *body, "{label}");
                match &finish.input {
                    BundleFinishInput::Incomplete => {
                        assert!(oracle.body.is_none(), "{label}");
                        checked_incomplete_bundles += 1;
                    }
                    BundleFinishInput::Complete { read, write, body } => {
                        let value = oracle
                            .body
                            .as_ref()
                            .unwrap_or_else(|| panic!("{label}: complete Bundle has no body"));
                        assert_eq!(*read, oracle.read_flag, "{label}");
                        assert_eq!(*write, oracle.write_flag, "{label}");
                        assert_eq!(
                            transcripts[body.index()],
                            authoritative_par_score(value),
                            "{label}"
                        );
                        checked_complete_bundles += 1;
                    }
                }
            }
            for finish in &trace.if_finishes {
                let body = &finish.body;
                rpb3_root_finish_trace::assert_if_finish_source(
                    finish,
                    source_entries,
                    &transcripts,
                    &label,
                );
                let branch = protobuf_decoder::decode_if(body.as_slice())
                    .unwrap_or_else(|error| panic!("{label}: If decode failed: {error}"));
                let oracle = If::decode(body.as_slice())
                    .unwrap_or_else(|error| panic!("{label}: If oracle decode failed: {error}"));
                assert_eq!(branch, oracle, "{label}");
                assert_eq!(oracle.encode_to_vec(), *body, "{label}");
                match &finish.input {
                    IfFinishInput::Incomplete => {
                        assert!(
                            oracle.condition.is_none()
                                || oracle.if_true.is_none()
                                || oracle.if_false.is_none(),
                            "{label}"
                        );
                        checked_incomplete_ifs += 1;
                    }
                    IfFinishInput::Complete {
                        condition,
                        if_true,
                        if_false,
                        connective_used,
                    } => {
                        for (handle, value) in [
                            (condition, oracle.condition.as_ref()),
                            (if_true, oracle.if_true.as_ref()),
                            (if_false, oracle.if_false.as_ref()),
                        ] {
                            let value = value.unwrap_or_else(|| {
                                panic!("{label}: complete If has a missing branch")
                            });
                            assert_eq!(
                                transcripts[handle.index()],
                                authoritative_par_score(value),
                                "{label}"
                            );
                        }
                        assert_eq!(*connective_used, oracle.connective_used, "{label}");
                        checked_complete_ifs += 1;
                    }
                }
            }
            for finish in &trace.match_finishes {
                let body = &finish.body;
                rpb3_root_finish_trace::assert_match_finish_source(
                    finish,
                    source_entries,
                    &transcripts,
                    &label,
                );
                let matched = protobuf_decoder::decode_match(body.as_slice())
                    .unwrap_or_else(|error| panic!("{label}: Match decode failed: {error}"));
                let oracle = Match::decode(body.as_slice())
                    .unwrap_or_else(|error| panic!("{label}: Match oracle decode failed: {error}"));
                assert_eq!(matched, oracle, "{label}");
                assert_eq!(oracle.encode_to_vec(), *body, "{label}");
                match &finish.input {
                    MatchFinishInput::Incomplete => {
                        assert!(
                            oracle.target.is_none()
                                || oracle.cases.iter().any(|case| {
                                    case.pattern.is_none() || case.source.is_none()
                                }),
                            "{label}"
                        );
                        checked_incomplete_matches += 1;
                    }
                    MatchFinishInput::Complete {
                        target,
                        cases,
                        connective_used,
                    } => {
                        let target_value = oracle
                            .target
                            .as_ref()
                            .unwrap_or_else(|| panic!("{label}: complete Match has no target"));
                        assert_eq!(
                            transcripts[target.index()],
                            authoritative_par_score(target_value),
                            "{label}"
                        );
                        assert_eq!(cases.len(), oracle.cases.len(), "{label}");
                        for (handle, case) in cases.iter().zip(&oracle.cases) {
                            assert_eq!(
                                transcripts[handle.index()],
                                authoritative_match_case_score(case),
                                "{label}"
                            );
                        }
                        assert_eq!(*connective_used, oracle.connective_used, "{label}");
                        checked_complete_matches += 1;
                    }
                }
            }
            checked += 1;
        }
        assert!(checked >= GENERATED_SCHEMA_TEST_MESSAGE_COUNT);
        assert!(checked_children > 0);
        assert!(checked_nested_pars > 0);
        assert!(checked_nested_epathmaps > 0);
        assert!(checked_driver_fields > 0);
        assert!(checked_driver_pops >= checked_children + checked);
        assert!(checked_complete_sends > 0);
        assert!(checked_incomplete_sends > 0);
        assert!(checked_complete_bundles > 0);
        assert!(checked_incomplete_bundles > 0);
        assert!(checked_complete_ifs > 0);
        assert!(checked_incomplete_ifs > 0);
        assert!(checked_complete_matches > 0);
        assert!(checked_incomplete_matches > 0);
        assert_eq!(component_counts.len(), 10);
        assert!(component_counts.values().all(|count| *count > 0));
    }

    #[test]
    fn nested_component_finish_matches_authoritative_body_and_score() {
        let expr = Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        };
        let value = Par::default().with_exprs(vec![expr.clone()]);
        let bytes = protobuf_encoder::encode_to_vec(&value);
        rpb3_root_finish_trace::start();
        validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&bytes),
            &PAR_PROTOBUF_REGION_PROGRAM,
        )
        .expect("canonical Par with a nested Expr is admitted");
        let trace = rpb3_root_finish_trace::take();
        let transcripts = rpb3_root_finish_trace::replay_score_source(
            trace
                .score_arena
                .as_ref()
                .expect("Par return captured its score arena"),
        );
        let child = trace
            .children
            .iter()
            .find(|child| std::ptr::eq(child.program, &EXPR_RHO_MATCH_PROGRAM))
            .expect("Par reduction observes its nested Expr finish");
        rpb3_root_finish_trace::assert_summary_score_source(
            &child.summary,
            &transcripts,
            "Par.exprs[0]",
        );
        assert_eq!(child.body, protobuf_encoder::encode_to_vec(&expr));
        let decoded =
            protobuf_decoder::decode_expr(child.body.as_slice()).expect("nested Expr body decodes");
        assert_nested_component_finish(&decoded, child, "Par.exprs[0]");
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
    fn message_root_rejects_a_copy_of_the_canonical_wire_program() {
        let expr = Expr {
            expr_instance: Some(ExprInstance::GBool(true)),
        };
        let bytes = protobuf_encoder::encode_to_vec(&expr);
        let canonical = &EXPR_RHO_MATCH_PROGRAM;
        let accepted =
            validate_canonical_protobuf(ProtobufRegion::copy_from_slice(&bytes), canonical);
        assert!(accepted.is_ok());

        let copied: &'static RhoMatchProgram = Box::leak(Box::new(*canonical));
        assert_eq!(copied, canonical);
        assert!(!std::ptr::eq(copied, canonical));
        let result = validate_canonical_protobuf(ProtobufRegion::copy_from_slice(&bytes), copied);
        assert!(matches!(
            result,
            Err(CanonicalProtobufAdmissionError::Schema {
                detail: "protobuf root is not the canonical wire program for its type ID",
                ..
            })
        ));
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
        let stats = admission.stats();
        assert_eq!(stats.epathmaps, 1);
        assert!(stats.epathmap_events >= 2);
        assert!(stats.messages >= 4);
        assert_component_score(&expr, &EXPR_RHO_MATCH_PROGRAM);

        let nested = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                [(gint(3), Par::default().with_exprs(vec![expr.clone()]))],
                Vec::new(),
                false,
                None,
            ))),
        };
        let nested_admission = validate(
            protobuf_encoder::encode_to_vec(&nested),
            &EXPR_RHO_MATCH_PROGRAM,
        );
        assert_eq!(nested_admission.stats().epathmaps, 2);
        assert!(nested_admission.stats().epathmap_events > stats.epathmap_events);
        assert_component_score(&nested, &EXPR_RHO_MATCH_PROGRAM);

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
                let finish = verified_monitor_seam::take_monitor_finish()
                    .expect("the deep admission reports its verified monitor finish");
                assert!(finish.accepted);
                assert_eq!(finish.checked_events, finish.iterations);
                let max_frames = u64::try_from(admission.stats().max_frames)
                    .expect("the deep admission frame count fits in u64");
                assert!(finish.checked_events + 1 >= 2 * max_frames);
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

#[cfg(test)]
mod verified_monitor_seam {
    use std::cell::Cell;

    use super::{CheckedChildSummaries, ObservedEvent, ObservedField};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum VerifiedMonitorFault {
        ZeroCursorAfter { step: u64 },
        UncheckedFrom { step: u64 },
        DropLastChildSummary,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct VerifiedMonitorFinish {
        pub(super) checked_events: u64,
        pub(super) iterations: u64,
        pub(super) accepted: bool,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(super) struct ChildSummariesRecord {
        pub(super) slot: usize,
        pub(super) bodies: Vec<Option<Vec<u8>>>,
    }

    struct SeamSession {
        fault: Option<VerifiedMonitorFault>,
        presented: Vec<ObservedEvent>,
        children: Vec<ChildSummariesRecord>,
    }

    thread_local! {
        static SESSION: Cell<Option<Box<SeamSession>>> = const { Cell::new(None) };
        static FINISH: Cell<Option<VerifiedMonitorFinish>> = const { Cell::new(None) };
    }

    pub(super) struct VerifiedMonitorSeamSession;

    impl VerifiedMonitorSeamSession {
        pub(super) fn begin(fault: Option<VerifiedMonitorFault>) -> Self {
            FINISH.with(|cell| cell.set(None));
            SESSION.with(|cell| {
                cell.set(Some(Box::new(SeamSession {
                    fault,
                    presented: Vec::new(),
                    children: Vec::new(),
                })))
            });
            Self
        }
    }

    impl Drop for VerifiedMonitorSeamSession {
        fn drop(&mut self) {
            FINISH.with(|cell| cell.set(None));
            SESSION.with(|cell| cell.set(None));
        }
    }

    fn with_session<T>(visit: impl FnOnce(Option<&mut SeamSession>) -> T) -> T {
        SESSION.with(|cell| {
            let mut session = cell.take();
            let visited = visit(session.as_deref_mut());
            cell.set(session);
            visited
        })
    }

    fn zero_cursor_after(event: ObservedEvent) -> ObservedEvent {
        let zeroed = |field: ObservedField| ObservedField {
            cursor_after: 0,
            ..field
        };
        match event {
            ObservedEvent::MessageContinue {
                before,
                field,
                after,
            } => ObservedEvent::MessageContinue {
                before,
                field: zeroed(field),
                after,
            },
            ObservedEvent::MessagePush {
                before,
                field,
                child,
                after,
            } => ObservedEvent::MessagePush {
                before,
                field: zeroed(field),
                child,
                after,
            },
            ObservedEvent::OtherContinue {
                before,
                field,
                after,
            } => ObservedEvent::OtherContinue {
                before,
                field: field.map(zeroed),
                after,
            },
            ObservedEvent::OtherPush {
                before,
                field,
                child,
                after,
            } => ObservedEvent::OtherPush {
                before,
                field: field.map(zeroed),
                child,
                after,
            },
            ObservedEvent::MessageComplete { .. } | ObservedEvent::OtherComplete { .. } => event,
        }
    }

    pub(super) fn seamed_event(step: u64, event: ObservedEvent) -> Option<ObservedEvent> {
        with_session(|session| {
            let Some(session) = session else {
                return Some(event);
            };
            let presented = match session.fault {
                Some(VerifiedMonitorFault::UncheckedFrom { step: first }) if step >= first => {
                    return None;
                }
                Some(VerifiedMonitorFault::ZeroCursorAfter { step: target }) if step == target => {
                    zero_cursor_after(event)
                }
                _ => event,
            };
            session.presented.push(presented);
            Some(presented)
        })
    }

    pub(super) fn seamed_children<'step, 'src>(
        children: Option<CheckedChildSummaries<'step, 'src>>,
    ) -> Option<CheckedChildSummaries<'step, 'src>> {
        with_session(|session| {
            let Some(session) = session else {
                return children;
            };
            let children = match (session.fault, children) {
                (
                    Some(VerifiedMonitorFault::DropLastChildSummary),
                    Some(CheckedChildSummaries::MessageSlot { slot, ordered }),
                ) => Some(CheckedChildSummaries::MessageSlot {
                    slot,
                    ordered: ordered.split_last().map_or(ordered, |(_, rest)| rest),
                }),
                (_, children) => children,
            };
            if let Some(CheckedChildSummaries::MessageSlot { slot, ordered }) = children {
                session.children.push(ChildSummariesRecord {
                    slot,
                    bodies: ordered
                        .iter()
                        .map(|occurrence| {
                            occurrence.body.as_ref().map(|body| body.bytes().collect())
                        })
                        .collect(),
                });
            }
            children
        })
    }

    pub(super) fn record_monitor_finish(checked_events: u64, iterations: u64, accepted: bool) {
        FINISH.with(|cell| {
            cell.set(Some(VerifiedMonitorFinish {
                checked_events,
                iterations,
                accepted,
            }))
        });
    }

    pub(super) fn take_monitor_finish() -> Option<VerifiedMonitorFinish> { FINISH.with(Cell::take) }

    pub(super) fn take_presented_events() -> Vec<ObservedEvent> {
        with_session(|session| {
            session
                .map(|session| std::mem::take(&mut session.presented))
                .unwrap_or_default()
        })
    }

    pub(super) fn take_child_summary_records() -> Vec<ChildSummariesRecord> {
        with_session(|session| {
            session
                .map(|session| std::mem::take(&mut session.children))
                .unwrap_or_default()
        })
    }
}

#[cfg(test)]
mod verified_monitor_call_site_tests {
    use std::collections::{BTreeMap, BTreeSet};

    use verified_score_store::admission_event_checker::MonitorRejection;

    use super::verified_monitor_seam::{
        take_child_summary_records, take_monitor_finish, take_presented_events,
        VerifiedMonitorFault, VerifiedMonitorFinish, VerifiedMonitorSeamSession,
    };
    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::{Expr, New, Par, Send};
    use crate::rust::canonical_path::{encode_trie_path, tag};
    use crate::rust::epathmap_trie_codec::{self, EPathMapRepr};
    use crate::rust::rhoapi_ext::EPathMap;
    use crate::rust::rholang::protobuf_encoder;

    const PRODUCTION_EVENT_CLASSES: [&str; 11] = [
        "MessageComplete.MapValue",
        "MessageComplete.MessageSlot",
        "MessageComplete.Reduced",
        "MessageComplete.Root",
        "MessageContinue",
        "MessagePush",
        "OtherComplete.MessageSlot",
        "OtherComplete.Reduced",
        "OtherComplete.Root",
        "OtherContinue",
        "OtherPush",
    ];

    fn observed_event_class(event: &ObservedEvent) -> &'static str {
        match event {
            ObservedEvent::MessageContinue { .. } => "MessageContinue",
            ObservedEvent::MessagePush { .. } => "MessagePush",
            ObservedEvent::MessageComplete { record, .. } => match record {
                ObservedAcceptRecord::Root => "MessageComplete.Root",
                ObservedAcceptRecord::MessageSlot { .. } => "MessageComplete.MessageSlot",
                ObservedAcceptRecord::MapValue => "MessageComplete.MapValue",
                ObservedAcceptRecord::Reduced => "MessageComplete.Reduced",
            },
            ObservedEvent::OtherContinue { .. } => "OtherContinue",
            ObservedEvent::OtherPush { .. } => "OtherPush",
            ObservedEvent::OtherComplete { record, .. } => match record {
                ObservedAcceptRecord::Root => "OtherComplete.Root",
                ObservedAcceptRecord::MessageSlot { .. } => "OtherComplete.MessageSlot",
                ObservedAcceptRecord::MapValue => "OtherComplete.MapValue",
                ObservedAcceptRecord::Reduced => "OtherComplete.Reduced",
            },
        }
    }

    fn fixture_len(len: usize) -> u64 { u64::try_from(len).expect("fixture lengths fit in u64") }

    fn integer(value: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }])
    }

    fn byte_array(bytes: &[u8]) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(bytes.to_vec())),
        }])
    }

    fn length_delimited_field(number: u32, body: &[u8]) -> Vec<u8> {
        let body_len = fixture_len(body.len());
        let mut field = Vec::with_capacity(
            prost::encoding::key_len(number)
                + prost::encoding::encoded_len_varint(body_len)
                + body.len(),
        );
        prost::encoding::encode_key(
            number,
            prost::encoding::WireType::LengthDelimited,
            &mut field,
        );
        prost::encoding::encode_varint(body_len, &mut field);
        field.extend_from_slice(body);
        field
    }

    fn escape_key() -> Vec<u8> {
        let raw = byte_array(&[37, 37, 41]).with_sends(vec![Send {
            chan: Some(integer(7)),
            ..Send::default()
        }]);
        let key = encode_trie_path(&raw);
        assert_eq!(key.first(), Some(&tag::ESCAPE));
        key
    }

    fn nested_epathmap_key() -> Vec<u8> {
        let child = EPathMap::new_map(
            [(integer(7), byte_array(&[37, 41]))],
            Vec::new(),
            false,
            None,
        );
        let snapshot = child.trie_snapshot();
        let snapshot_len = fixture_len(snapshot.len());
        let mut key = Vec::with_capacity(
            1 + prost::encoding::encoded_len_varint(snapshot_len) + snapshot.len(),
        );
        key.push(tag::EPATHMAP_MAP);
        prost::encoding::encode_varint(snapshot_len, &mut key);
        key.extend_from_slice(snapshot);
        key
    }

    fn epathmap_par_with_key(key: &[u8]) -> Vec<u8> {
        let mut trie = pathmap::PathMap::new();
        trie.insert(key, integer(11));
        let snapshot = epathmap_trie_codec::encode(&EPathMapRepr::Map(trie));
        length_delimited_field(
            5,
            &length_delimited_field(32, &length_delimited_field(9, &snapshot)),
        )
    }

    fn gint_expr_bytes(value: i64) -> Vec<u8> {
        protobuf_encoder::encode_to_vec(&Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        })
    }

    fn admit_message(
        bytes: Vec<u8>,
        program: &'static RhoMatchProgram,
    ) -> Result<CanonicalProtobufAdmission, CanonicalProtobufAdmissionError> {
        validate_canonical_protobuf(ProtobufRegion::from(bytes), program)
    }

    fn classes_of_an_admitted_run<T>(
        admit: impl FnOnce() -> Result<T, CanonicalProtobufAdmissionError>,
    ) -> BTreeSet<&'static str> {
        let _session = VerifiedMonitorSeamSession::begin(None);
        if let Err(error) = admit() {
            panic!("a base-admitted fixture was refused: {error}");
        }
        let presented = take_presented_events();
        let finish = take_monitor_finish().expect("an admitted run reports its monitor finish");
        assert!(finish.accepted);
        assert_eq!(finish.checked_events, finish.iterations);
        assert_eq!(fixture_len(presented.len()), finish.checked_events);
        presented.iter().map(observed_event_class).collect()
    }

    #[test]
    fn every_production_event_class_is_checked_once_per_iteration() {
        let expr = gint_expr_bytes(7);
        let new = protobuf_encoder::encode_to_vec(&New {
            p: Some(integer(1)),
            injections: BTreeMap::from([("k".to_string(), integer(2))]),
            ..New::default()
        });
        let escape = escape_key();
        let nested = nested_epathmap_key();
        let escape_par = epathmap_par_with_key(&escape);
        let nested_par = epathmap_par_with_key(&nested);
        let mut classes = BTreeSet::new();
        classes.extend(classes_of_an_admitted_run(|| {
            admit_message(expr, &EXPR_RHO_MATCH_PROGRAM)
        }));
        classes.extend(classes_of_an_admitted_run(|| {
            admit_message(new, &NEW_RHO_MATCH_PROGRAM)
        }));
        classes.extend(classes_of_an_admitted_run(|| {
            admit_message(escape_par, &PAR_PROTOBUF_REGION_PROGRAM)
        }));
        classes.extend(classes_of_an_admitted_run(|| {
            admit_message(nested_par, &PAR_PROTOBUF_REGION_PROGRAM)
        }));
        classes.extend(classes_of_an_admitted_run(|| {
            validate_canonical_path_indexed(IndexedByteRegion::from_borrowed(&escape, 0, 0))
        }));
        classes.extend(classes_of_an_admitted_run(|| {
            validate_canonical_path_indexed(IndexedByteRegion::from_borrowed(&nested, 0, 0))
        }));
        assert_eq!(classes, BTreeSet::from(PRODUCTION_EVENT_CLASSES));
    }

    #[test]
    fn a_refused_event_ends_the_admission_with_a_typed_refusal() {
        let bytes = gint_expr_bytes(7);
        {
            let _session = VerifiedMonitorSeamSession::begin(None);
            assert!(admit_message(bytes.clone(), &EXPR_RHO_MATCH_PROGRAM).is_ok());
            let classes: Vec<&str> = take_presented_events()
                .iter()
                .map(observed_event_class)
                .collect();
            assert_eq!(classes, ["MessageContinue", "MessageComplete.Root"]);
            assert_eq!(
                take_monitor_finish(),
                Some(VerifiedMonitorFinish {
                    checked_events: 2,
                    iterations: 2,
                    accepted: true,
                })
            );
        }
        let _session =
            VerifiedMonitorSeamSession::begin(Some(VerifiedMonitorFault::ZeroCursorAfter {
                step: 0,
            }));
        assert_eq!(
            admit_message(bytes, &EXPR_RHO_MATCH_PROGRAM).err(),
            Some(CanonicalProtobufAdmissionError::VerifiedMonitorRefused {
                step: 0,
                refusal: MonitorRefusal::Rejected(MonitorRejection::FieldRefused),
            })
        );
        assert!(matches!(take_presented_events().as_slice(), [
            ObservedEvent::MessageContinue {
                field: ObservedField {
                    cursor_after: 0,
                    ..
                },
                ..
            }
        ]));
        assert_eq!(take_monitor_finish(), None);
    }

    #[test]
    fn completions_carry_the_ordered_child_summaries_of_their_slot() {
        let values = [11, 22, 33];
        let bodies: Vec<Vec<u8>> = values.iter().copied().map(gint_expr_bytes).collect();
        let bytes = protobuf_encoder::encode_to_vec(
            &Par::default().with_exprs(
                values
                    .iter()
                    .map(|value| Expr {
                        expr_instance: Some(ExprInstance::GInt(*value)),
                    })
                    .collect(),
            ),
        );
        let _session = VerifiedMonitorSeamSession::begin(None);
        assert!(admit_message(bytes, &PAR_PROTOBUF_REGION_PROGRAM).is_ok());
        let records = take_child_summary_records();
        assert_eq!(records.len(), values.len());
        let slot = records[0].slot;
        for (index, record) in records.iter().enumerate() {
            assert_eq!(record.slot, slot);
            let expected: Vec<Option<Vec<u8>>> =
                bodies[..=index].iter().cloned().map(Some).collect();
            assert_eq!(record.bodies, expected);
        }
        let accepted: Vec<(usize, usize)> = take_presented_events()
            .iter()
            .filter_map(|event| match event {
                ObservedEvent::MessageComplete {
                    record:
                        ObservedAcceptRecord::MessageSlot {
                            slot,
                            occurrences_after,
                        },
                    ..
                } => Some((*slot, *occurrences_after)),
                _ => None,
            })
            .collect();
        assert_eq!(accepted, [(slot, 1), (slot, 2), (slot, 3)]);
    }

    #[test]
    fn an_admission_with_unchecked_iterations_fails_closed() {
        let bytes = gint_expr_bytes(7);
        for (first_unchecked, checked_events) in [(0, 0), (1, 1)] {
            let _session =
                VerifiedMonitorSeamSession::begin(Some(VerifiedMonitorFault::UncheckedFrom {
                    step: first_unchecked,
                }));
            assert_eq!(
                admit_message(bytes.clone(), &EXPR_RHO_MATCH_PROGRAM).err(),
                Some(CanonicalProtobufAdmissionError::VerifiedMonitorIncomplete { checked_events })
            );
            assert_eq!(take_monitor_finish(), None);
        }
    }

    #[test]
    fn summaries_that_disagree_with_the_acceptance_receipt_are_refused() {
        let bytes = protobuf_encoder::encode_to_vec(&integer(5));
        let _session =
            VerifiedMonitorSeamSession::begin(Some(VerifiedMonitorFault::DropLastChildSummary));
        assert_eq!(
            admit_message(bytes, &PAR_PROTOBUF_REGION_PROGRAM).err(),
            Some(CanonicalProtobufAdmissionError::CutoverInvariant(
                "checked admission event disagrees with its accepted child summaries"
            ))
        );
        let classes: Vec<&str> = take_presented_events()
            .iter()
            .map(observed_event_class)
            .collect();
        assert_eq!(classes, ["MessagePush", "MessageContinue"]);
        assert_eq!(take_monitor_finish(), None);
    }
}
