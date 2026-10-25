//! Schema-driven RMP2 events over provenance-admitted protobuf regions.
//!
//! A resident `ParBag` endpoint already contains the canonical protobuf body
//! of its component. This cursor borrows that body while the PathMap zipper is
//! live and interprets only protobuf framing and generated schema metadata. It
//! never constructs a recursive rhoapi value. Nested `Par` messages remain one
//! opaque, multiplicity-compressed component transcript plus their two root
//! metadata fields.

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(test)]
use std::sync::Mutex;

use crypto::rust::hash::blake2b256::Blake2b256;

use super::match_path::{
    append_par_bag_summary, feed_par_bag_match_record, ParBagMatchSummary, ABSENT, BOOL, BYTES,
    BYTES_SEQUENCE, FIELD_END, FIELD_START, I32, I64, MAP_END, MAP_KEY, MAP_START, NODE_END,
    NODE_START, ONEOF_END, ONEOF_START, PAR_BAG_MATCH_TRANSCRIPT_DOMAIN, PATHMAP,
    PATHMAP_MATCH_DOMAIN, PATHMAP_MATCH_EDGE, PATHMAP_MATCH_LEAVE, RHO_MATCH_PATH_VERSION,
    SEQUENCE_END, SEQUENCE_START, STRING, STRING_SEQUENCE, U32, U64,
};
use super::match_schema::{
    protobuf_field_at_slot, resolve_protobuf_field, ResolvedProtobufField, ResolvedProtobufPayload,
    RhoMatchField, RhoMatchFieldKind, RhoMatchOneofVariant, RhoMatchPayloadKind, RhoMatchProgram,
};
use super::match_schema_tables::{
    EXPR_RHO_MATCH_PROGRAM, PAR_PROTOBUF_REGION_PROGRAM, PAR_RHO_MATCH_PROGRAM,
};
use super::protobuf_cursor::{CanonicalField, CanonicalFieldCursor, CanonicalFieldValue};
use super::protobuf_region::{require_wire_program, verify_protobuf_wire, ProtobufRegionError};
use super::protobuf_schema::ProtobufKind;
use crate::rhoapi::Expr;
use crate::rust::epathmap_trie_codec::{
    visit_canonical_snapshot_topology, CanonicalSnapshotTopologyEvent, EPathMapMode,
};
use crate::rust::par_bag::{canonical_exact_hole_expr, CanonicalExactHole, EncodedParComponent};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanonicalRhoRegionScalar<'a> {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Bytes(&'a [u8]),
    String(&'a str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathMapMatchSummary {
    mode: EPathMapMode,
    len: u64,
    digest: [u8; 32],
}

/// Borrowed proof-carrying view of a nested canonical `Par` region.
///
/// Fields stay private: this handle can only be emitted while walking a
/// component admitted through `EncodedParComponent`. Query compilation can
/// safely classify or descend it without gaining an arbitrary-byte bypass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmittedParRegion<'a> {
    bytes: &'a [u8],
    sole_expr: Option<&'a [u8]>,
    locally_free: &'a [u8],
    connective_used: bool,
}

/// Borrowed proof that an admitted nested `Par` is a metadata-free singleton
/// Expr carrier. Construction is deliberately private to this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmittedExprRegion<'a> {
    bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmittedParRegionClass<'a> {
    ExactFreeVar(i32),
    ExactWildcard,
    MetadataBearingWholeValue,
    SoleExpr(AdmittedExprRegion<'a>),
    Opaque,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmittedExprRegionClass<'a> {
    ExactFreeVar(i32),
    ExactWildcard,
    Structural(AdmittedExprRegion<'a>),
}

/// Classify the authoritative outer Expr before an ordered-query compiler
/// decides whether to emit a whole-value hole or consume structural events.
pub fn classify_admitted_expr<'a>(
    component: &EncodedParComponent<'a, Expr>,
) -> AdmittedExprRegionClass<'a> {
    match canonical_exact_hole_expr(component.payload()) {
        Some(CanonicalExactHole::FreeVar(level)) => AdmittedExprRegionClass::ExactFreeVar(level),
        Some(CanonicalExactHole::Wildcard) => AdmittedExprRegionClass::ExactWildcard,
        None => AdmittedExprRegionClass::Structural(AdmittedExprRegion {
            bytes: component.payload(),
        }),
    }
}

impl<'a> AdmittedParRegion<'a> {
    pub const fn locally_free(self) -> &'a [u8] { self.locally_free }

    pub const fn connective_used(self) -> bool { self.connective_used }

    /// Apply the exact distinctions needed before an ordered query suppresses
    /// the opaque ParBag token. A negative result never means mismatch; it
    /// means query compilation must keep the token or conservatively fall back.
    pub fn classify(self) -> AdmittedParRegionClass<'a> {
        let Some(expr) = self.sole_expr else {
            return if self.connective_used || !self.locally_free.is_empty() {
                AdmittedParRegionClass::MetadataBearingWholeValue
            } else {
                AdmittedParRegionClass::Opaque
            };
        };
        if let Some(hole) = canonical_exact_hole_expr(expr) {
            return match hole {
                CanonicalExactHole::FreeVar(level) => AdmittedParRegionClass::ExactFreeVar(level),
                CanonicalExactHole::Wildcard => AdmittedParRegionClass::ExactWildcard,
            };
        }
        if self.connective_used || !self.locally_free.is_empty() {
            AdmittedParRegionClass::MetadataBearingWholeValue
        } else {
            AdmittedParRegionClass::SoleExpr(AdmittedExprRegion { bytes: expr })
        }
    }

    fn summary(self) -> Result<ParBagMatchSummary, ProtobufRegionError> {
        #[cfg(test)]
        PAR_REGION_SUMMARY_HASHES.fetch_add(1, Ordering::Relaxed);
        Ok(canonical_par_view(self.bytes)?.summary)
    }
}

#[cfg(test)]
static PAR_REGION_SUMMARY_HASHES: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static PAR_REGION_SUMMARY_TEST_LOCK: Mutex<()> = Mutex::new(());

impl PathMapMatchSummary {
    pub const fn mode(self) -> EPathMapMode { self.mode }

    pub const fn len(self) -> u64 { self.len }

    pub const fn is_empty(self) -> bool { self.len == 0 }

    pub const fn digest(self) -> [u8; 32] { self.digest }
}

/// One semantic event in the canonical protobuf-region RMP2 walk.
///
/// Scalar repeated fields have explicit start/element/end events because a
/// borrowed protobuf run is not a resident `Vec<String>`/`Vec<Vec<u8>>`.
/// Their RMP2 byte image is nevertheless identical to the generated resident
/// cursor's single sequence scalar token.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanonicalRhoPathRegionEvent<'a> {
    EnterNode(&'static RhoMatchProgram),
    LeaveNode,
    EnterField(&'static RhoMatchField),
    LeaveField,
    Scalar(CanonicalRhoRegionScalar<'a>),
    Absent,
    EnterSequence(u64),
    LeaveSequence,
    EnterMap(u64),
    MapKey(&'a str),
    LeaveMap,
    EnterOneof(&'static RhoMatchOneofVariant),
    LeaveOneof,
    EnterStringSequence(u64),
    StringSequenceElement(&'a str),
    LeaveStringSequence,
    EnterBytesSequence(u64),
    BytesSequenceElement(&'a [u8]),
    LeaveBytesSequence,
    ParBag(AdmittedParRegion<'a>),
    PathMap(PathMapMatchSummary),
}

#[derive(Clone, Copy)]
struct MessageState<'a> {
    program: &'static RhoMatchProgram,
    fields: CanonicalFieldCursor<'a>,
    pending: Option<CanonicalField<'a>>,
    last_slot: Option<usize>,
}

impl<'a> MessageState<'a> {
    fn new(message: &'a [u8], program: &'static RhoMatchProgram) -> Self {
        Self {
            program,
            fields: CanonicalFieldCursor::in_prost_slot_order(message),
            pending: None,
            last_slot: None,
        }
    }

    fn peek_slot(&mut self) -> Result<Option<usize>, ProtobufRegionError> {
        if self.pending.is_none() {
            let Some(field) = self.fields.next_field()? else {
                return Ok(None);
            };
            let resolved = resolve_protobuf_field(self.program, field.number).ok_or(
                ProtobufRegionError::Schema {
                    type_name: self.program.type_name,
                    field: field.number,
                    detail: "field is outside the generated schema",
                },
            )?;
            verify_protobuf_wire(
                self.program.type_name,
                field.number,
                field.encoded_range.start,
                field.wire_type,
                resolved.protobuf_kind(),
            )?;
            let slot = resolved.slot();
            if self.last_slot.is_some_and(|last| slot < last) {
                return Err(ProtobufRegionError::Schema {
                    type_name: self.program.type_name,
                    field: field.number,
                    detail: "field is outside canonical prost slot order",
                });
            }
            self.last_slot = Some(slot);
            self.pending = Some(field);
        }
        Ok(self.pending.map(|field| {
            resolve_protobuf_field(self.program, field.number)
                .expect("peek resolved the generated field")
                .slot()
        }))
    }

    fn take_slot(&mut self, slot: usize) -> Result<CanonicalField<'a>, ProtobufRegionError> {
        if self.peek_slot()? != Some(slot) {
            return Err(ProtobufRegionError::Schema {
                type_name: self.program.type_name,
                field: self.program.fields[slot].tag,
                detail: "generated slot occurrence is missing",
            });
        }
        Ok(self
            .pending
            .take()
            .expect("peek loaded the requested field"))
    }

    fn scan_slot(mut self, slot: usize) -> Result<(u64, Self), ProtobufRegionError> {
        let mut count = 0u64;
        while self.peek_slot()? == Some(slot) {
            self.pending = None;
            count = count
                .checked_add(1)
                .ok_or(ProtobufRegionError::SizeOverflow)?;
        }
        Ok((count, self))
    }
}

#[derive(Clone, Copy)]
enum ScalarSequenceKind {
    String,
    Bytes,
}

enum Work<'a> {
    Emit(CanonicalRhoPathRegionEvent<'a>),
    Message {
        state: MessageState<'a>,
        next_field: usize,
        entered: bool,
    },
    Value {
        state: MessageState<'a>,
        slot: usize,
        count: u64,
        resolved: ResolvedProtobufField,
    },
    RepeatedMessage {
        state: MessageState<'a>,
        slot: usize,
        remaining: u64,
        child: &'static RhoMatchProgram,
    },
    ScalarSequence {
        state: MessageState<'a>,
        slot: usize,
        remaining: u64,
        total: u64,
        kind: ScalarSequenceKind,
        entered: bool,
    },
    Map {
        state: MessageState<'a>,
        slot: usize,
        remaining: u64,
        child: &'static RhoMatchProgram,
    },
    Par(&'a [u8]),
}

pub struct CanonicalRhoPathRegionCursor<'a> {
    work: Vec<Work<'a>>,
    failed: bool,
}

impl<'a> CanonicalRhoPathRegionCursor<'a> {
    /// Enter the region route through an authoritative resident RPB3 endpoint.
    /// `EncodedParComponent` has private fields and can only be minted by the
    /// validated ParBag visitor, so arbitrary protobuf bytes cannot bypass
    /// canonical admission through this constructor.
    pub fn admitted_expr(component: EncodedParComponent<'a, Expr>) -> Self {
        Self::admitted_nested_expr(AdmittedExprRegion {
            bytes: component.payload(),
        })
    }

    /// Continue an ordered region walk beneath a proven nested singleton Expr.
    pub fn admitted_nested_expr(region: AdmittedExprRegion<'a>) -> Self {
        Self {
            work: vec![Work::Message {
                state: MessageState::new(region.bytes, &EXPR_RHO_MATCH_PROGRAM),
                next_field: 0,
                entered: false,
            }],
            failed: false,
        }
    }

    fn child_work(program: &'static RhoMatchProgram, payload: &'a [u8]) -> Work<'a> {
        if program.type_id == PAR_RHO_MATCH_PROGRAM.type_id {
            Work::Par(payload)
        } else {
            Work::Message {
                state: MessageState::new(payload, program),
                next_field: 0,
                entered: false,
            }
        }
    }

    fn next_event(
        &mut self,
    ) -> Result<Option<CanonicalRhoPathRegionEvent<'a>>, ProtobufRegionError> {
        loop {
            let Some(work) = self.work.pop() else {
                return Ok(None);
            };
            match work {
                Work::Emit(event) => return Ok(Some(event)),
                Work::Message {
                    mut state,
                    next_field,
                    entered,
                } => {
                    if !entered {
                        let program = state.program;
                        self.work.push(Work::Message {
                            state,
                            next_field,
                            entered: true,
                        });
                        return Ok(Some(CanonicalRhoPathRegionEvent::EnterNode(program)));
                    }
                    if next_field == state.program.fields.len() {
                        let remaining = match state.pending {
                            Some(field) => Some(field),
                            None => state.fields.next_field()?,
                        };
                        if let Some(field) = remaining {
                            return Err(ProtobufRegionError::Schema {
                                type_name: state.program.type_name,
                                field: field.number,
                                detail: "field remains after the generated schema program",
                            });
                        }
                        return Ok(Some(CanonicalRhoPathRegionEvent::LeaveNode));
                    }

                    let mut run_start = state;
                    let (count, after) = state.scan_slot(next_field)?;
                    let resolved = match run_start.peek_slot()? {
                        Some(slot) if slot == next_field => resolve_protobuf_field(
                            run_start.program,
                            run_start.pending.expect("peek loaded a field").number,
                        )
                        .expect("peek resolved the generated field"),
                        _ => protobuf_field_at_slot(run_start.program, next_field)
                            .expect("the generated message walk names a valid field slot"),
                    };
                    let field = &state.program.fields[next_field];
                    self.work.push(Work::Message {
                        state: after,
                        next_field: next_field + 1,
                        entered: true,
                    });
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::LeaveField));
                    self.work.push(Work::Value {
                        state: run_start,
                        slot: next_field,
                        count,
                        resolved,
                    });
                    return Ok(Some(CanonicalRhoPathRegionEvent::EnterField(field)));
                }
                Work::Value {
                    mut state,
                    slot,
                    count,
                    resolved,
                } => {
                    let field = resolved.field();
                    match field.kind {
                        RhoMatchFieldKind::Bool
                        | RhoMatchFieldKind::I32
                        | RhoMatchFieldKind::U32
                        | RhoMatchFieldKind::I64
                        | RhoMatchFieldKind::U64
                        | RhoMatchFieldKind::Bytes
                        | RhoMatchFieldKind::String => {
                            require_singular(state.program, field.tag, count)?;
                            let scalar = if count == 0 {
                                default_scalar(field.kind)
                            } else {
                                let encoded = state.take_slot(slot)?;
                                decode_scalar(field.kind, resolved.protobuf_kind(), encoded)?
                            };
                            return Ok(Some(CanonicalRhoPathRegionEvent::Scalar(scalar)));
                        }
                        RhoMatchFieldKind::StringSequence | RhoMatchFieldKind::BytesSequence => {
                            let kind = match field.kind {
                                RhoMatchFieldKind::StringSequence => ScalarSequenceKind::String,
                                RhoMatchFieldKind::BytesSequence => ScalarSequenceKind::Bytes,
                                _ => unreachable!(),
                            };
                            self.work.push(Work::ScalarSequence {
                                state,
                                slot,
                                remaining: count,
                                total: count,
                                kind,
                                entered: false,
                            });
                        }
                        RhoMatchFieldKind::OptionalMessage(type_id) => {
                            require_singular(state.program, field.tag, count)?;
                            if count == 0 {
                                return Ok(Some(CanonicalRhoPathRegionEvent::Absent));
                            }
                            let encoded = state.take_slot(slot)?;
                            let payload = encoded.expect_length_delimited()?;
                            let child = require_wire_program(state.program, field.tag, type_id)?;
                            self.work.push(Self::child_work(child, payload));
                        }
                        RhoMatchFieldKind::MessageSequence(type_id) => {
                            let child = require_wire_program(state.program, field.tag, type_id)?;
                            self.work
                                .push(Work::Emit(CanonicalRhoPathRegionEvent::LeaveSequence));
                            self.work.push(Work::RepeatedMessage {
                                state,
                                slot,
                                remaining: count,
                                child,
                            });
                            return Ok(Some(CanonicalRhoPathRegionEvent::EnterSequence(count)));
                        }
                        RhoMatchFieldKind::StringMessageMap(type_id) => {
                            let child = require_wire_program(state.program, field.tag, type_id)?;
                            self.work
                                .push(Work::Emit(CanonicalRhoPathRegionEvent::LeaveMap));
                            self.work.push(Work::Map {
                                state,
                                slot,
                                remaining: count,
                                child,
                            });
                            return Ok(Some(CanonicalRhoPathRegionEvent::EnterMap(count)));
                        }
                        RhoMatchFieldKind::Oneof(_) => {
                            require_singular(state.program, field.tag, count)?;
                            if count == 0 {
                                return Ok(Some(CanonicalRhoPathRegionEvent::Absent));
                            }
                            let ResolvedProtobufPayload::Oneof(variant) = resolved.payload() else {
                                return Err(ProtobufRegionError::Schema {
                                    type_name: state.program.type_name,
                                    field: field.tag,
                                    detail: "present oneof slot did not resolve an arm",
                                });
                            };
                            let encoded = state.take_slot(slot)?;
                            self.work
                                .push(Work::Emit(CanonicalRhoPathRegionEvent::LeaveOneof));
                            match variant.payload {
                                RhoMatchPayloadKind::Message(type_id) => {
                                    let child =
                                        require_wire_program(state.program, variant.tag, type_id)?;
                                    self.work.push(Self::child_work(
                                        child,
                                        encoded.expect_length_delimited()?,
                                    ));
                                }
                                payload => {
                                    let scalar = decode_oneof_scalar(
                                        payload,
                                        variant.protobuf_kind,
                                        encoded,
                                    )?;
                                    self.work.push(Work::Emit(
                                        CanonicalRhoPathRegionEvent::Scalar(scalar),
                                    ));
                                }
                            }
                            return Ok(Some(CanonicalRhoPathRegionEvent::EnterOneof(variant)));
                        }
                        RhoMatchFieldKind::PathMap => {
                            require_singular(state.program, field.tag, count)?;
                            if count != 1 {
                                return Err(ProtobufRegionError::Schema {
                                    type_name: state.program.type_name,
                                    field: field.tag,
                                    detail: "canonical EPathMap snapshot is absent",
                                });
                            }
                            let encoded = state.take_slot(slot)?;
                            let summary =
                                canonical_pathmap_summary(encoded.expect_length_delimited()?)?;
                            return Ok(Some(CanonicalRhoPathRegionEvent::PathMap(summary)));
                        }
                        RhoMatchFieldKind::ParBag => {
                            return Err(ProtobufRegionError::Schema {
                                type_name: state.program.type_name,
                                field: field.tag,
                                detail: "native ParBag field appeared in a protobuf message",
                            });
                        }
                    }
                }
                Work::RepeatedMessage {
                    mut state,
                    slot,
                    remaining,
                    child,
                } => {
                    if remaining == 0 {
                        continue;
                    }
                    let encoded = state.take_slot(slot)?;
                    self.work.push(Work::RepeatedMessage {
                        state,
                        slot,
                        remaining: remaining - 1,
                        child,
                    });
                    self.work
                        .push(Self::child_work(child, encoded.expect_length_delimited()?));
                }
                Work::ScalarSequence {
                    mut state,
                    slot,
                    remaining,
                    total,
                    kind,
                    entered,
                } => {
                    if !entered {
                        self.work.push(Work::ScalarSequence {
                            state,
                            slot,
                            remaining,
                            total,
                            kind,
                            entered: true,
                        });
                        return Ok(Some(match kind {
                            ScalarSequenceKind::String => {
                                CanonicalRhoPathRegionEvent::EnterStringSequence(total)
                            }
                            ScalarSequenceKind::Bytes => {
                                CanonicalRhoPathRegionEvent::EnterBytesSequence(total)
                            }
                        }));
                    }
                    if remaining == 0 {
                        return Ok(Some(match kind {
                            ScalarSequenceKind::String => {
                                CanonicalRhoPathRegionEvent::LeaveStringSequence
                            }
                            ScalarSequenceKind::Bytes => {
                                CanonicalRhoPathRegionEvent::LeaveBytesSequence
                            }
                        }));
                    }
                    let encoded = state.take_slot(slot)?;
                    let payload = encoded.expect_length_delimited()?;
                    self.work.push(Work::ScalarSequence {
                        state,
                        slot,
                        remaining: remaining - 1,
                        total,
                        kind,
                        entered: true,
                    });
                    return Ok(Some(match kind {
                        ScalarSequenceKind::String => {
                            CanonicalRhoPathRegionEvent::StringSequenceElement(
                                std::str::from_utf8(payload).map_err(|_| {
                                    ProtobufRegionError::Schema {
                                        type_name: "protobuf string sequence",
                                        field: encoded.number,
                                        detail: "string payload is not UTF-8",
                                    }
                                })?,
                            )
                        }
                        ScalarSequenceKind::Bytes => {
                            CanonicalRhoPathRegionEvent::BytesSequenceElement(payload)
                        }
                    }));
                }
                Work::Map {
                    mut state,
                    slot,
                    remaining,
                    child,
                } => {
                    if remaining == 0 {
                        continue;
                    }
                    let encoded = state.take_slot(slot)?;
                    let (key, value) = parse_map_entry(encoded.expect_length_delimited()?)?;
                    self.work.push(Work::Map {
                        state,
                        slot,
                        remaining: remaining - 1,
                        child,
                    });
                    self.work.push(Self::child_work(child, value));
                    return Ok(Some(CanonicalRhoPathRegionEvent::MapKey(key)));
                }
                Work::Par(bytes) => {
                    let shell = canonical_par_shell(bytes)?;
                    let [components, locally_free, connective_used] =
                        validated_par_match_fields(&PAR_RHO_MATCH_PROGRAM)?;
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::LeaveNode));
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::LeaveField));
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::Scalar(
                            CanonicalRhoRegionScalar::Bool(shell.connective_used),
                        )));
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::EnterField(
                            connective_used,
                        )));
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::LeaveField));
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::Scalar(
                            CanonicalRhoRegionScalar::Bytes(shell.locally_free),
                        )));
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::EnterField(
                            locally_free,
                        )));
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::LeaveField));
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::ParBag(
                            AdmittedParRegion {
                                bytes,
                                sole_expr: shell.sole_expr,
                                locally_free: shell.locally_free,
                                connective_used: shell.connective_used,
                            },
                        )));
                    self.work
                        .push(Work::Emit(CanonicalRhoPathRegionEvent::EnterField(
                            components,
                        )));
                    return Ok(Some(CanonicalRhoPathRegionEvent::EnterNode(
                        &PAR_RHO_MATCH_PROGRAM,
                    )));
                }
            }
        }
    }
}

fn validated_par_match_fields(
    program: &'static RhoMatchProgram,
) -> Result<[&'static RhoMatchField; 3], ProtobufRegionError> {
    let [components, locally_free, connective_used] = program.fields else {
        return Err(ProtobufRegionError::Schema {
            type_name: program.type_name,
            field: 0,
            detail: "generated Par match program does not have exactly three fields",
        });
    };
    if components.tag != 0
        || components.name != "components"
        || !matches!(components.kind, RhoMatchFieldKind::ParBag)
    {
        return Err(ProtobufRegionError::Schema {
            type_name: program.type_name,
            field: components.tag,
            detail: "generated Par component field is incompatible with RMP2",
        });
    }
    if locally_free.tag != 9
        || locally_free.name != "locally_free"
        || !matches!(locally_free.kind, RhoMatchFieldKind::Bytes)
    {
        return Err(ProtobufRegionError::Schema {
            type_name: program.type_name,
            field: locally_free.tag,
            detail: "generated Par locally_free field is incompatible with RMP2",
        });
    }
    if connective_used.tag != 10
        || connective_used.name != "connective_used"
        || !matches!(connective_used.kind, RhoMatchFieldKind::Bool)
    {
        return Err(ProtobufRegionError::Schema {
            type_name: program.type_name,
            field: connective_used.tag,
            detail: "generated Par connective_used field is incompatible with RMP2",
        });
    }
    Ok([components, locally_free, connective_used])
}

impl<'a> Iterator for CanonicalRhoPathRegionCursor<'a> {
    type Item = Result<CanonicalRhoPathRegionEvent<'a>, ProtobufRegionError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.next_event() {
            Ok(Some(event)) => Some(Ok(event)),
            Ok(None) => None,
            Err(error) => {
                self.failed = true;
                Some(Err(error))
            }
        }
    }
}

fn require_singular(
    program: &'static RhoMatchProgram,
    field: u32,
    count: u64,
) -> Result<(), ProtobufRegionError> {
    if count <= 1 {
        Ok(())
    } else {
        Err(ProtobufRegionError::Schema {
            type_name: program.type_name,
            field,
            detail: "canonical singular field is duplicated",
        })
    }
}

fn default_scalar(kind: RhoMatchFieldKind) -> CanonicalRhoRegionScalar<'static> {
    match kind {
        RhoMatchFieldKind::Bool => CanonicalRhoRegionScalar::Bool(false),
        RhoMatchFieldKind::I32 => CanonicalRhoRegionScalar::I32(0),
        RhoMatchFieldKind::U32 => CanonicalRhoRegionScalar::U32(0),
        RhoMatchFieldKind::I64 => CanonicalRhoRegionScalar::I64(0),
        RhoMatchFieldKind::U64 => CanonicalRhoRegionScalar::U64(0),
        RhoMatchFieldKind::Bytes => CanonicalRhoRegionScalar::Bytes(&[]),
        RhoMatchFieldKind::String => CanonicalRhoRegionScalar::String(""),
        _ => unreachable!("only scalar field kinds have scalar defaults"),
    }
}

fn decode_oneof_scalar<'a>(
    payload: RhoMatchPayloadKind,
    protobuf: ProtobufKind,
    field: CanonicalField<'a>,
) -> Result<CanonicalRhoRegionScalar<'a>, ProtobufRegionError> {
    let kind = match payload {
        RhoMatchPayloadKind::Bool => RhoMatchFieldKind::Bool,
        RhoMatchPayloadKind::I32 => RhoMatchFieldKind::I32,
        RhoMatchPayloadKind::U32 => RhoMatchFieldKind::U32,
        RhoMatchPayloadKind::I64 => RhoMatchFieldKind::I64,
        RhoMatchPayloadKind::U64 => RhoMatchFieldKind::U64,
        RhoMatchPayloadKind::Bytes => RhoMatchFieldKind::Bytes,
        RhoMatchPayloadKind::String => RhoMatchFieldKind::String,
        RhoMatchPayloadKind::Message(_) => unreachable!("message oneof handled separately"),
    };
    decode_scalar(kind, protobuf, field)
}

fn decode_scalar<'a>(
    kind: RhoMatchFieldKind,
    protobuf: ProtobufKind,
    field: CanonicalField<'a>,
) -> Result<CanonicalRhoRegionScalar<'a>, ProtobufRegionError> {
    let schema = |detail| ProtobufRegionError::Schema {
        type_name: "protobuf scalar",
        field: field.number,
        detail,
    };
    Ok(match (kind, protobuf, field.value) {
        (RhoMatchFieldKind::Bool, ProtobufKind::Bool, CanonicalFieldValue::Varint(value)) => {
            CanonicalRhoRegionScalar::Bool(match value {
                0 => false,
                1 => true,
                _ => return Err(schema("boolean value is not zero or one")),
            })
        }
        (RhoMatchFieldKind::I32, ProtobufKind::Int32, CanonicalFieldValue::Varint(value)) => {
            CanonicalRhoRegionScalar::I32(value as u32 as i32)
        }
        (RhoMatchFieldKind::I32, ProtobufKind::Sint32, CanonicalFieldValue::Varint(value)) => {
            let value = u32::try_from(value).map_err(|_| schema("sint32 value overflows u32"))?;
            CanonicalRhoRegionScalar::I32(((value >> 1) as i32) ^ -((value & 1) as i32))
        }
        (RhoMatchFieldKind::I32, ProtobufKind::Sfixed32, CanonicalFieldValue::Fixed32(value)) => {
            CanonicalRhoRegionScalar::I32(i32::from_le_bytes(value.try_into().unwrap()))
        }
        (RhoMatchFieldKind::U32, ProtobufKind::Uint32, CanonicalFieldValue::Varint(value)) => {
            CanonicalRhoRegionScalar::U32(
                u32::try_from(value).map_err(|_| schema("uint32 value overflows u32"))?,
            )
        }
        (RhoMatchFieldKind::U32, ProtobufKind::Fixed32, CanonicalFieldValue::Fixed32(value)) => {
            CanonicalRhoRegionScalar::U32(u32::from_le_bytes(value.try_into().unwrap()))
        }
        (RhoMatchFieldKind::I64, ProtobufKind::Int64, CanonicalFieldValue::Varint(value)) => {
            CanonicalRhoRegionScalar::I64(value as i64)
        }
        (RhoMatchFieldKind::I64, ProtobufKind::Sint64, CanonicalFieldValue::Varint(value)) => {
            CanonicalRhoRegionScalar::I64(((value >> 1) as i64) ^ -((value & 1) as i64))
        }
        (RhoMatchFieldKind::I64, ProtobufKind::Sfixed64, CanonicalFieldValue::Fixed64(value)) => {
            CanonicalRhoRegionScalar::I64(i64::from_le_bytes(value.try_into().unwrap()))
        }
        (RhoMatchFieldKind::U64, ProtobufKind::Uint64, CanonicalFieldValue::Varint(value)) => {
            CanonicalRhoRegionScalar::U64(value)
        }
        (RhoMatchFieldKind::U64, ProtobufKind::Fixed64, CanonicalFieldValue::Fixed64(value)) => {
            CanonicalRhoRegionScalar::U64(u64::from_le_bytes(value.try_into().unwrap()))
        }
        (
            RhoMatchFieldKind::Bytes,
            ProtobufKind::Bytes,
            CanonicalFieldValue::LengthDelimited(value),
        ) => CanonicalRhoRegionScalar::Bytes(value),
        (
            RhoMatchFieldKind::String,
            ProtobufKind::String,
            CanonicalFieldValue::LengthDelimited(value),
        ) => CanonicalRhoRegionScalar::String(
            std::str::from_utf8(value).map_err(|_| schema("string payload is not UTF-8"))?,
        ),
        _ => return Err(schema("generated scalar and protobuf kinds disagree")),
    })
}

fn parse_map_entry(bytes: &[u8]) -> Result<(&str, &[u8]), ProtobufRegionError> {
    let mut fields = CanonicalFieldCursor::new(bytes);
    let mut key = None;
    let mut value = None;
    while let Some(field) = fields.next_field()? {
        match field.number {
            1 if key.is_none() => {
                let payload = field.expect_length_delimited()?;
                key = Some(std::str::from_utf8(payload).map_err(|_| {
                    ProtobufRegionError::Schema {
                        type_name: "protobuf map entry",
                        field: 1,
                        detail: "map key is not UTF-8",
                    }
                })?);
            }
            2 if value.is_none() => value = Some(field.expect_length_delimited()?),
            1 | 2 => {
                return Err(ProtobufRegionError::Schema {
                    type_name: "protobuf map entry",
                    field: field.number,
                    detail: "map entry field is duplicated",
                });
            }
            _ => {
                return Err(ProtobufRegionError::Schema {
                    type_name: "protobuf map entry",
                    field: field.number,
                    detail: "map entry field is outside the generated schema",
                });
            }
        }
    }
    Ok((key.unwrap_or(""), value.unwrap_or(&[])))
}

struct CanonicalParView {
    summary: ParBagMatchSummary,
}

struct CanonicalParShell<'a> {
    sole_expr: Option<&'a [u8]>,
    locally_free: &'a [u8],
    connective_used: bool,
}

const fn saturating_region_cardinality_increment(value: u64) -> u64 { value.saturating_add(1) }

/// Validate only the canonical top-level Par envelope. Length-delimited
/// component bodies remain borrowed and skipped in O(1); this is the lazy
/// classifier path used by dynamic-query scans. The proportional RMP2 digest
/// is deliberately deferred until the ParBag token is actually emitted.
fn canonical_par_shell(bytes: &[u8]) -> Result<CanonicalParShell<'_>, ProtobufRegionError> {
    u64::try_from(bytes.len()).map_err(|_| ProtobufRegionError::SizeOverflow)?;
    let mut cursor = CanonicalFieldCursor::new(bytes);
    let mut component_count = 0u64;
    let mut sole_expr = None;
    let mut locally_free = None;
    let mut connective_used = false;

    while let Some(field) = cursor.next_field()? {
        let resolved = resolve_protobuf_field(&PAR_PROTOBUF_REGION_PROGRAM, field.number).ok_or(
            ProtobufRegionError::Schema {
                type_name: PAR_PROTOBUF_REGION_PROGRAM.type_name,
                field: field.number,
                detail: "field is outside the generated Par schema",
            },
        )?;
        verify_protobuf_wire(
            PAR_PROTOBUF_REGION_PROGRAM.type_name,
            field.number,
            field.encoded_range.start,
            field.wire_type,
            resolved.protobuf_kind(),
        )?;
        let schema_field = resolved.field();
        if matches!(schema_field.kind, RhoMatchFieldKind::MessageSequence(_)) {
            let payload = field.expect_length_delimited()?;
            component_count = saturating_region_cardinality_increment(component_count);
            sole_expr = if component_count == 1 && schema_field.name == "exprs" {
                Some(payload)
            } else {
                None
            };
            continue;
        }
        match schema_field.name {
            "locally_free" if locally_free.is_none() => {
                let payload = field.expect_length_delimited()?;
                if payload.is_empty() {
                    return Err(ProtobufRegionError::Schema {
                        type_name: "Par",
                        field: field.number,
                        detail: "canonical locally_free default is present",
                    });
                }
                locally_free = Some(payload);
            }
            "connective_used" if !connective_used => {
                if field.expect_varint()? != 1 {
                    return Err(ProtobufRegionError::Schema {
                        type_name: "Par",
                        field: field.number,
                        detail: "canonical connective_used value is not true",
                    });
                }
                connective_used = true;
            }
            "locally_free" | "connective_used" => {
                return Err(ProtobufRegionError::Schema {
                    type_name: "Par",
                    field: field.number,
                    detail: "canonical metadata field is duplicated",
                });
            }
            _ => {
                return Err(ProtobufRegionError::Schema {
                    type_name: "Par",
                    field: field.number,
                    detail: "generated Par field is not a component or metadata",
                });
            }
        }
    }
    Ok(CanonicalParShell {
        sole_expr: (component_count == 1).then_some(sole_expr).flatten(),
        locally_free: locally_free.unwrap_or(&[]),
        connective_used,
    })
}

fn canonical_par_view(bytes: &[u8]) -> Result<CanonicalParView, ProtobufRegionError> {
    // Every encoded field consumes at least its key byte. Proving that the
    // admitted allocation length fits u64 is therefore a strict upper bound
    // on occurrence count and matches ParBag's saturating-u64 cardinality.
    u64::try_from(bytes.len()).map_err(|_| ProtobufRegionError::SizeOverflow)?;
    let mut locally_free = None;
    let mut connective_used = false;
    let mut distinct = 0u64;
    let mut cardinality = 0u64;
    let mut failure = None;

    let digest = Blake2b256::hash_stream(|feed| {
        feed(PAR_BAG_MATCH_TRANSCRIPT_DOMAIN);
        let mut cursor = CanonicalFieldCursor::new(bytes);
        let mut previous: Option<(u32, &[u8], u64)> = None;
        let flush = |feed: &mut dyn FnMut(&[u8]),
                     previous: &mut Option<(u32, &[u8], u64)>,
                     distinct: &mut u64| {
            if let Some((tag, payload, multiplicity)) = previous.take() {
                feed_par_bag_match_record(feed, tag, multiplicity, payload);
                *distinct = saturating_region_cardinality_increment(*distinct);
            }
        };

        loop {
            let field = match cursor.next_field() {
                Ok(Some(field)) => field,
                Ok(None) => break,
                Err(error) => {
                    failure = Some(error.into());
                    break;
                }
            };
            let Some(resolved) = resolve_protobuf_field(&PAR_PROTOBUF_REGION_PROGRAM, field.number)
            else {
                failure = Some(ProtobufRegionError::Schema {
                    type_name: PAR_PROTOBUF_REGION_PROGRAM.type_name,
                    field: field.number,
                    detail: "field is outside the generated Par schema",
                });
                break;
            };
            if let Err(error) = verify_protobuf_wire(
                PAR_PROTOBUF_REGION_PROGRAM.type_name,
                field.number,
                field.encoded_range.start,
                field.wire_type,
                resolved.protobuf_kind(),
            ) {
                failure = Some(error);
                break;
            }
            let schema_field = resolved.field();
            if matches!(schema_field.kind, RhoMatchFieldKind::MessageSequence(_)) {
                let payload = match field.expect_length_delimited() {
                    Ok(payload) => payload,
                    Err(error) => {
                        failure = Some(error.into());
                        break;
                    }
                };
                cardinality = saturating_region_cardinality_increment(cardinality);
                match previous.as_mut() {
                    Some((tag, previous_payload, multiplicity))
                        if *tag == field.number && *previous_payload == payload =>
                    {
                        *multiplicity = saturating_region_cardinality_increment(*multiplicity);
                    }
                    _ => {
                        flush(feed, &mut previous, &mut distinct);
                        previous = Some((field.number, payload, 1));
                    }
                }
                continue;
            }

            match schema_field.name {
                "locally_free" if locally_free.is_none() => match field.expect_length_delimited() {
                    Ok(payload) if !payload.is_empty() => locally_free = Some(payload),
                    Ok(_) => {
                        failure = Some(ProtobufRegionError::Schema {
                            type_name: "Par",
                            field: field.number,
                            detail: "canonical locally_free default is present",
                        });
                        break;
                    }
                    Err(error) => {
                        failure = Some(error.into());
                        break;
                    }
                },
                "connective_used" if !connective_used => match field.expect_varint() {
                    Ok(1) => connective_used = true,
                    Ok(_) => {
                        failure = Some(ProtobufRegionError::Schema {
                            type_name: "Par",
                            field: field.number,
                            detail: "canonical connective_used value is not true",
                        });
                        break;
                    }
                    Err(error) => {
                        failure = Some(error.into());
                        break;
                    }
                },
                "locally_free" | "connective_used" => {
                    failure = Some(ProtobufRegionError::Schema {
                        type_name: "Par",
                        field: field.number,
                        detail: "canonical metadata field is duplicated",
                    });
                    break;
                }
                _ => {
                    failure = Some(ProtobufRegionError::Schema {
                        type_name: "Par",
                        field: field.number,
                        detail: "field is outside the generated Par schema",
                    });
                    break;
                }
            }
        }
        if failure.is_none() {
            flush(feed, &mut previous, &mut distinct);
        }
    });

    if let Some(error) = failure {
        return Err(error);
    }
    Ok(CanonicalParView {
        summary: ParBagMatchSummary::new(
            distinct,
            cardinality,
            digest
                .try_into()
                .expect("BLAKE2b-256 always returns exactly 32 bytes"),
        ),
    })
}

fn canonical_pathmap_summary(bytes: &[u8]) -> Result<PathMapMatchSummary, ProtobufRegionError> {
    let mut result = None;
    let mut failure = None;
    let digest = Blake2b256::hash_stream(|feed| {
        feed(PATHMAP_MATCH_DOMAIN);
        match visit_canonical_snapshot_topology(bytes, |event| match event {
            CanonicalSnapshotTopologyEvent::Value(has_value) => {
                feed(&[u8::from(has_value)]);
            }
            CanonicalSnapshotTopologyEvent::Edge(byte) => feed(&[PATHMAP_MATCH_EDGE, byte]),
            CanonicalSnapshotTopologyEvent::Leave => feed(&[PATHMAP_MATCH_LEAVE]),
        }) {
            Ok(info) => result = Some(info),
            Err(error) => failure = Some(error),
        }
    });
    if let Some(error) = failure {
        return Err(error.into());
    }
    let (mode, len) = result.expect("successful topology traversal records its mode and length");
    Ok(PathMapMatchSummary {
        mode,
        len: u64::try_from(len).map_err(|_| ProtobufRegionError::SizeOverflow)?,
        digest: digest
            .try_into()
            .expect("BLAKE2b-256 always returns exactly 32 bytes"),
    })
}

pub fn encode_admitted_expr_match_path(
    component: EncodedParComponent<'_, Expr>,
) -> Result<Vec<u8>, ProtobufRegionError> {
    let mut output = Vec::with_capacity(256);
    output.extend_from_slice(&RHO_MATCH_PATH_VERSION);
    for event in CanonicalRhoPathRegionCursor::admitted_expr(component) {
        append_region_event(&mut output, event?)?;
    }
    Ok(output)
}

pub fn append_region_event(
    output: &mut Vec<u8>,
    event: CanonicalRhoPathRegionEvent<'_>,
) -> Result<(), ProtobufRegionError> {
    match event {
        CanonicalRhoPathRegionEvent::EnterNode(program) => {
            output.push(NODE_START);
            output.extend_from_slice(&program.type_id.to_be_bytes());
        }
        CanonicalRhoPathRegionEvent::LeaveNode => output.push(NODE_END),
        CanonicalRhoPathRegionEvent::EnterField(field) => {
            output.push(FIELD_START);
            output.extend_from_slice(&field.tag.to_be_bytes());
        }
        CanonicalRhoPathRegionEvent::LeaveField => output.push(FIELD_END),
        CanonicalRhoPathRegionEvent::Scalar(value) => append_region_scalar(output, value),
        CanonicalRhoPathRegionEvent::Absent => output.push(ABSENT),
        CanonicalRhoPathRegionEvent::EnterSequence(len) => {
            output.push(SEQUENCE_START);
            output.extend_from_slice(&len.to_be_bytes());
        }
        CanonicalRhoPathRegionEvent::LeaveSequence => output.push(SEQUENCE_END),
        CanonicalRhoPathRegionEvent::EnterMap(len) => {
            output.push(MAP_START);
            output.extend_from_slice(&len.to_be_bytes());
        }
        CanonicalRhoPathRegionEvent::MapKey(key) => {
            output.push(MAP_KEY);
            append_bytes(output, key.as_bytes());
        }
        CanonicalRhoPathRegionEvent::LeaveMap => output.push(MAP_END),
        CanonicalRhoPathRegionEvent::EnterOneof(variant) => {
            output.push(ONEOF_START);
            output.extend_from_slice(&variant.tag.to_be_bytes());
        }
        CanonicalRhoPathRegionEvent::LeaveOneof => output.push(ONEOF_END),
        CanonicalRhoPathRegionEvent::EnterStringSequence(len) => {
            output.push(STRING_SEQUENCE);
            output.extend_from_slice(&len.to_be_bytes());
        }
        CanonicalRhoPathRegionEvent::StringSequenceElement(value) => {
            append_bytes(output, value.as_bytes());
        }
        CanonicalRhoPathRegionEvent::LeaveStringSequence => {}
        CanonicalRhoPathRegionEvent::EnterBytesSequence(len) => {
            output.push(BYTES_SEQUENCE);
            output.extend_from_slice(&len.to_be_bytes());
        }
        CanonicalRhoPathRegionEvent::BytesSequenceElement(value) => append_bytes(output, value),
        CanonicalRhoPathRegionEvent::LeaveBytesSequence => {}
        CanonicalRhoPathRegionEvent::ParBag(region) => {
            append_par_bag_summary(output, region.summary()?)
        }
        CanonicalRhoPathRegionEvent::PathMap(summary) => {
            output.push(PATHMAP);
            output.push(summary.mode as u8);
            output.extend_from_slice(&summary.len.to_be_bytes());
            output.extend_from_slice(&summary.digest);
        }
    }
    Ok(())
}

fn append_region_scalar(output: &mut Vec<u8>, value: CanonicalRhoRegionScalar<'_>) {
    match value {
        CanonicalRhoRegionScalar::Bool(value) => output.extend_from_slice(&[BOOL, value.into()]),
        CanonicalRhoRegionScalar::I32(value) => {
            output.push(I32);
            output.extend_from_slice(&value.to_be_bytes());
        }
        CanonicalRhoRegionScalar::U32(value) => {
            output.push(U32);
            output.extend_from_slice(&value.to_be_bytes());
        }
        CanonicalRhoRegionScalar::I64(value) => {
            output.push(I64);
            output.extend_from_slice(&value.to_be_bytes());
        }
        CanonicalRhoRegionScalar::U64(value) => {
            output.push(U64);
            output.extend_from_slice(&value.to_be_bytes());
        }
        CanonicalRhoRegionScalar::Bytes(value) => {
            output.push(BYTES);
            append_bytes(output, value);
        }
        CanonicalRhoRegionScalar::String(value) => {
            output.push(STRING);
            append_bytes(output, value.as_bytes());
        }
    }
}

fn append_bytes(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(
        &u64::try_from(value.len())
            .expect("an allocated RMP2 value length fits in u64")
            .to_be_bytes(),
    );
    output.extend_from_slice(value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::{EList, Expr, Par};
    use crate::rust::rholang::protobuf_encoder;

    static SHORT_PAR_FIELDS: &[RhoMatchField] = &[];
    static SHORT_PAR_PROGRAM: RhoMatchProgram = RhoMatchProgram {
        type_id: 1,
        type_name: "Par",
        fields: SHORT_PAR_FIELDS,
        protobuf_fields: &[],
    };
    static WRONG_PAR_FIELDS: &[RhoMatchField] = &[
        RhoMatchField {
            tag: 0,
            name: "components",
            kind: RhoMatchFieldKind::ParBag,
        },
        RhoMatchField {
            tag: 9,
            name: "locally_free",
            kind: RhoMatchFieldKind::Bytes,
        },
        RhoMatchField {
            tag: 11,
            name: "connective_used",
            kind: RhoMatchFieldKind::Bool,
        },
    ];
    static WRONG_PAR_PROGRAM: RhoMatchProgram = RhoMatchProgram {
        type_id: 1,
        type_name: "Par",
        fields: WRONG_PAR_FIELDS,
        protobuf_fields: &[],
    };

    #[test]
    fn generated_par_program_drift_is_a_schema_error_not_an_indexing_panic() {
        let [components, locally_free, connective_used] =
            validated_par_match_fields(&PAR_RHO_MATCH_PROGRAM).unwrap();
        assert_eq!(components.name, "components");
        assert_eq!(locally_free.name, "locally_free");
        assert_eq!(connective_used.name, "connective_used");

        assert_eq!(
            validated_par_match_fields(&SHORT_PAR_PROGRAM),
            Err(ProtobufRegionError::Schema {
                type_name: "Par",
                field: 0,
                detail: "generated Par match program does not have exactly three fields",
            })
        );
        assert_eq!(
            validated_par_match_fields(&WRONG_PAR_PROGRAM),
            Err(ProtobufRegionError::Schema {
                type_name: "Par",
                field: 11,
                detail: "generated Par connective_used field is incompatible with RMP2",
            })
        );
    }

    #[test]
    fn malformed_par_shells_are_refused_before_classification() {
        assert!(canonical_par_shell(&[0x7A, 0]).is_err());
        assert!(canonical_par_shell(&[0x4A, 0]).is_err());
        assert!(canonical_par_shell(&[0x50, 1, 0x50, 1]).is_err());
        assert!(canonical_par_shell(&[0x48, 1]).is_err());
    }

    #[test]
    fn conflicting_expr_oneof_arms_are_refused_before_event_completion() {
        // GBool(true), followed by GInt(1): both tags resolve to the one
        // generated `expr_instance` slot, which is canonically singular.
        let region = AdmittedExprRegion {
            bytes: &[0x08, 0x01, 0x10, 0x02],
        };
        let error = CanonicalRhoPathRegionCursor::admitted_nested_expr(region)
            .find_map(Result::err)
            .expect("the conflicting oneof arms must fail closed");
        assert_eq!(error, ProtobufRegionError::Schema {
            type_name: "Expr",
            field: 1,
            detail: "canonical singular field is duplicated",
        });
    }

    #[test]
    fn par_region_summary_is_lazy_until_the_opaque_token_is_emitted() {
        let _guard = PAR_REGION_SUMMARY_TEST_LOCK.lock().unwrap();
        let nested = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }]);
        let bytes = protobuf_encoder::encode_to_vec(&nested);
        let shell = canonical_par_shell(&bytes).unwrap();
        let region = AdmittedParRegion {
            bytes: &bytes,
            sole_expr: shell.sole_expr,
            locally_free: shell.locally_free,
            connective_used: shell.connective_used,
        };

        PAR_REGION_SUMMARY_HASHES.store(0, Ordering::Relaxed);
        assert!(matches!(
            region.classify(),
            AdmittedParRegionClass::SoleExpr(_)
        ));
        assert_eq!(PAR_REGION_SUMMARY_HASHES.load(Ordering::Relaxed), 0);

        let mut encoded = Vec::new();
        append_region_event(&mut encoded, CanonicalRhoPathRegionEvent::ParBag(region)).unwrap();
        assert_eq!(PAR_REGION_SUMMARY_HASHES.load(Ordering::Relaxed), 1);
        assert_eq!(encoded.len(), 49);
    }

    #[test]
    fn region_cardinality_projection_saturates_at_u64_boundary() {
        assert_eq!(
            saturating_region_cardinality_increment(u64::MAX - 1),
            u64::MAX
        );
        assert_eq!(saturating_region_cardinality_increment(u64::MAX), u64::MAX);
    }

    #[test]
    fn deep_dynamic_scan_can_classify_shells_without_requesting_summaries() {
        let _guard = PAR_REGION_SUMMARY_TEST_LOCK.lock().unwrap();
        let nested = Par::wrap_unary_elist(
            1_024,
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EVarBody(crate::rhoapi::EVar {
                    v: Some(crate::rhoapi::Var {
                        var_instance: Some(crate::rhoapi::var::VarInstance::FreeVar(0)),
                    }),
                })),
            }]),
        )
        .expect("canonical unary-list fixture is representable");
        let outer = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![nested],
                locally_free: Vec::new(),
                connective_used: true,
                remainder: None,
            })),
        }]);
        PAR_REGION_SUMMARY_HASHES.store(0, Ordering::Relaxed);
        outer.visit_encoded::<Expr>(|component| {
            let AdmittedExprRegionClass::Structural(root) = classify_admitted_expr(&component)
            else {
                panic!("outer list is structural");
            };
            let mut scans = vec![CanonicalRhoPathRegionCursor::admitted_nested_expr(root)];
            while let Some(scan) = scans.last_mut() {
                let Some(event) = scan.next() else {
                    scans.pop();
                    continue;
                };
                if let CanonicalRhoPathRegionEvent::ParBag(region) = event.unwrap() {
                    if let AdmittedParRegionClass::SoleExpr(child) = region.classify() {
                        scans.push(CanonicalRhoPathRegionCursor::admitted_nested_expr(child));
                    }
                }
            }
        });
        assert_eq!(
            PAR_REGION_SUMMARY_HASHES.load(Ordering::Relaxed),
            0,
            "classification-only traversal must not hash any proportional Par suffix"
        );
    }
}
