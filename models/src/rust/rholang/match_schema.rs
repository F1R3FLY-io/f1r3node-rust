use std::collections::BTreeMap;

use super::match_schema_tables::{
    ParCursor, ParSequenceCursor, PAR_PROTOBUF_REGION_PROGRAM, RHO_MATCH_REGISTRY,
};
use super::protobuf_schema::{ProtobufField, ProtobufKind};
use crate::rhoapi::Par;
use crate::rust::par_bag::ParBag;
use crate::rust::rhoapi_ext::EPathMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhoMatchFieldKind {
    Bool,
    I32,
    U32,
    I64,
    U64,
    Bytes,
    String,
    StringSequence,
    BytesSequence,
    OptionalMessage(u16),
    MessageSequence(u16),
    StringMessageMap(u16),
    Oneof(&'static [RhoMatchOneofVariant]),
    ParBag,
    PathMap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhoMatchPayloadKind {
    Bool,
    I32,
    U32,
    I64,
    U64,
    Bytes,
    String,
    Message(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RhoMatchOneofVariant {
    pub tag: u32,
    pub name: &'static str,
    pub payload: RhoMatchPayloadKind,
    pub protobuf_kind: ProtobufKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RhoMatchField {
    pub tag: u32,
    pub name: &'static str,
    pub kind: RhoMatchFieldKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RhoMatchProgram {
    pub type_id: u16,
    pub type_name: &'static str,
    pub fields: &'static [RhoMatchField],
    pub protobuf_fields: &'static [ProtobufField],
}

/// The concrete protobuf payload selected for one generated field slot.
///
/// `Declared` is also used for an absent oneof slot, where the program carries
/// the [`ProtobufKind::Oneof`] marker but no arm has appeared on the wire.
/// `Oneof` records the selected arm and therefore its concrete protobuf kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolvedProtobufPayload {
    Declared(ProtobufKind),
    Oneof(&'static RhoMatchOneofVariant),
}

/// One actual protobuf tag resolved to its generated Prost field slot.
///
/// The fields stay private so callers cannot manufacture schema provenance;
/// instances come only from [`resolve_protobuf_field`] or
/// [`protobuf_field_at_slot`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedProtobufField {
    slot: usize,
    field: &'static RhoMatchField,
    payload: ResolvedProtobufPayload,
}

impl ResolvedProtobufField {
    #[inline]
    pub(crate) const fn slot(self) -> usize { self.slot }

    #[inline]
    pub(crate) const fn field(self) -> &'static RhoMatchField { self.field }

    #[inline]
    pub(crate) const fn payload(self) -> ResolvedProtobufPayload { self.payload }

    #[inline]
    pub(crate) const fn protobuf_kind(self) -> ProtobufKind {
        match self.payload {
            ResolvedProtobufPayload::Declared(kind) => kind,
            ResolvedProtobufPayload::Oneof(variant) => variant.protobuf_kind,
        }
    }

    #[inline]
    pub(crate) const fn repeated(self) -> bool {
        matches!(
            self.field.kind,
            RhoMatchFieldKind::StringSequence
                | RhoMatchFieldKind::BytesSequence
                | RhoMatchFieldKind::MessageSequence(_)
                | RhoMatchFieldKind::StringMessageMap(_)
        )
    }
}

/// Resolve one actual wire tag through the generated Prost slot program.
///
/// A oneof occupies the slot of its minimum declared tag, while the selected
/// arm writes its own tag. Returning both the slot and selected arm keeps that
/// distinction in one descriptor-derived authority.
pub(crate) fn resolve_protobuf_field(
    program: &'static RhoMatchProgram,
    tag: u32,
) -> Option<ResolvedProtobufField> {
    debug_assert_eq!(program.fields.len(), program.protobuf_fields.len());
    for (slot, (field, protobuf)) in program
        .fields
        .iter()
        .zip(program.protobuf_fields)
        .enumerate()
    {
        if let RhoMatchFieldKind::Oneof(variants) = field.kind {
            if let Some(variant) = variants.iter().find(|variant| variant.tag == tag) {
                return Some(ResolvedProtobufField {
                    slot,
                    field,
                    payload: ResolvedProtobufPayload::Oneof(variant),
                });
            }
        } else if protobuf.tag == tag {
            return Some(ResolvedProtobufField {
                slot,
                field,
                payload: ResolvedProtobufPayload::Declared(protobuf.kind),
            });
        }
    }
    None
}

/// Resolve one generated slot even when no field occurrence selected it.
pub(crate) fn protobuf_field_at_slot(
    program: &'static RhoMatchProgram,
    slot: usize,
) -> Option<ResolvedProtobufField> {
    debug_assert_eq!(program.fields.len(), program.protobuf_fields.len());
    Some(ResolvedProtobufField {
        slot,
        field: program.fields.get(slot)?,
        payload: ResolvedProtobufPayload::Declared(program.protobuf_fields.get(slot)?.kind),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WireProgramLookupError {
    ZeroTypeId,
    OutsideRegistry,
    DoesNotIndexSelf,
}

impl WireProgramLookupError {
    pub(crate) const fn detail(self) -> &'static str {
        match self {
            Self::ZeroTypeId => "child type identifier is zero",
            Self::OutsideRegistry => "child type identifier is outside the generated registry",
            Self::DoesNotIndexSelf => "child type identifier does not index itself",
        }
    }
}

/// Resolve a generated type identifier to its raw-wire program.
///
/// The registry's `Par` row deliberately describes the resident native bag.
/// Raw protobuf traversal must instead use the descriptor-accurate external
/// program emitted from the same decoder field list.
pub(crate) fn wire_program_by_type_id(
    type_id: u16,
) -> Result<&'static RhoMatchProgram, WireProgramLookupError> {
    if type_id == 0 {
        return Err(WireProgramLookupError::ZeroTypeId);
    }
    if type_id == PAR_PROTOBUF_REGION_PROGRAM.type_id {
        return Ok(&PAR_PROTOBUF_REGION_PROGRAM);
    }
    let (_, program) = RHO_MATCH_REGISTRY
        .get(usize::from(type_id) - 1)
        .ok_or(WireProgramLookupError::OutsideRegistry)?;
    if program.type_id != type_id {
        return Err(WireProgramLookupError::DoesNotIndexSelf);
    }
    Ok(*program)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RhoScalarRef<'a> {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Bytes(&'a [u8]),
    String(&'a str),
    StringSequence(&'a [String]),
    BytesSequence(&'a [Vec<u8>]),
}

#[derive(Clone, Copy)]
pub enum RhoOneofPayload<'a> {
    Scalar(RhoScalarRef<'a>),
    Message(ParCursor<'a>),
}

#[derive(Clone, Copy)]
pub struct RhoOneofCursor<'a> {
    pub oneof_name: &'static str,
    pub variant_name: &'static str,
    pub serde_index: u32,
    pub protobuf_tag: u32,
    pub payload: RhoOneofPayload<'a>,
}

#[derive(Clone, Copy)]
pub enum RhoMatchValue<'a> {
    Scalar(RhoScalarRef<'a>),
    OptionalMessage(Option<ParCursor<'a>>),
    MessageSequence(ParSequenceCursor<'a>),
    StringMessageMap(&'a BTreeMap<String, Par>),
    Oneof(Option<RhoOneofCursor<'a>>),
    ParBag(&'a ParBag),
    PathMap(&'a EPathMap),
}

#[derive(Clone, Copy)]
pub enum RhoPathEvent<'a> {
    EnterNode {
        cursor: ParCursor<'a>,
        program: &'static RhoMatchProgram,
    },
    LeaveNode,
    EnterField(&'static RhoMatchField),
    LeaveField,
    Scalar(RhoScalarRef<'a>),
    Absent,
    EnterSequence(usize),
    LeaveSequence,
    EnterMap(usize),
    MapKey(&'a str),
    LeaveMap,
    EnterOneof(RhoOneofCursor<'a>),
    LeaveOneof,
    ParBag(&'a ParBag),
    PathMap(&'a EPathMap),
}

enum RhoPathStep<'a> {
    Emit(RhoPathEvent<'a>),
    Node {
        cursor: ParCursor<'a>,
        field: usize,
    },
    Value(RhoMatchValue<'a>),
    Sequence {
        cursor: ParSequenceCursor<'a>,
        index: usize,
    },
    Map {
        entries: std::collections::btree_map::Iter<'a, String, Par>,
    },
    Oneof(RhoOneofCursor<'a>),
}

pub struct RhoPathCursor<'a> {
    work: Vec<RhoPathStep<'a>>,
}

impl<'a> RhoPathCursor<'a> {
    pub fn new(root: ParCursor<'a>) -> Self {
        Self {
            work: vec![RhoPathStep::Node {
                cursor: root,
                field: 0,
            }],
        }
    }

    pub fn par(root: &'a Par) -> Self { Self::new(ParCursor::Par(root)) }
}

impl<'a> Iterator for RhoPathCursor<'a> {
    type Item = RhoPathEvent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.work.pop()? {
                RhoPathStep::Emit(event) => return Some(event),
                RhoPathStep::Node { cursor, field } => {
                    if field == 0 {
                        self.work.push(RhoPathStep::Emit(RhoPathEvent::LeaveNode));
                    }
                    let meta = cursor.program().fields.get(field);
                    if field == 0 && meta.is_none() {
                        return Some(RhoPathEvent::EnterNode {
                            cursor,
                            program: cursor.program(),
                        });
                    }
                    let Some(meta) = meta else { continue };
                    let value = cursor
                        .field(field)
                        .expect("generated cursor program and accessor must have equal lengths");
                    self.work.push(RhoPathStep::Node {
                        cursor,
                        field: field + 1,
                    });
                    self.work.push(RhoPathStep::Emit(RhoPathEvent::LeaveField));
                    self.work.push(RhoPathStep::Value(value));
                    self.work
                        .push(RhoPathStep::Emit(RhoPathEvent::EnterField(meta)));
                    if field == 0 {
                        return Some(RhoPathEvent::EnterNode {
                            cursor,
                            program: cursor.program(),
                        });
                    }
                }
                RhoPathStep::Value(value) => match value {
                    RhoMatchValue::Scalar(value) => return Some(RhoPathEvent::Scalar(value)),
                    RhoMatchValue::OptionalMessage(Some(cursor)) => {
                        self.work.push(RhoPathStep::Node { cursor, field: 0 });
                    }
                    RhoMatchValue::OptionalMessage(None) => return Some(RhoPathEvent::Absent),
                    RhoMatchValue::MessageSequence(cursor) => {
                        self.work
                            .push(RhoPathStep::Emit(RhoPathEvent::LeaveSequence));
                        self.work.push(RhoPathStep::Sequence { cursor, index: 0 });
                        return Some(RhoPathEvent::EnterSequence(cursor.len()));
                    }
                    RhoMatchValue::StringMessageMap(entries) => {
                        self.work.push(RhoPathStep::Emit(RhoPathEvent::LeaveMap));
                        self.work.push(RhoPathStep::Map {
                            entries: entries.iter(),
                        });
                        return Some(RhoPathEvent::EnterMap(entries.len()));
                    }
                    RhoMatchValue::Oneof(Some(cursor)) => {
                        self.work.push(RhoPathStep::Emit(RhoPathEvent::LeaveOneof));
                        self.work.push(RhoPathStep::Oneof(cursor));
                        return Some(RhoPathEvent::EnterOneof(cursor));
                    }
                    RhoMatchValue::Oneof(None) => return Some(RhoPathEvent::Absent),
                    RhoMatchValue::ParBag(bag) => return Some(RhoPathEvent::ParBag(bag)),
                    RhoMatchValue::PathMap(map) => return Some(RhoPathEvent::PathMap(map)),
                },
                RhoPathStep::Sequence { cursor, index } => {
                    let Some(child) = cursor.get(index) else {
                        continue;
                    };
                    self.work.push(RhoPathStep::Sequence {
                        cursor,
                        index: index + 1,
                    });
                    self.work.push(RhoPathStep::Node {
                        cursor: child,
                        field: 0,
                    });
                }
                RhoPathStep::Map { mut entries } => {
                    let Some((key, value)) = entries.next() else {
                        continue;
                    };
                    self.work.push(RhoPathStep::Map { entries });
                    self.work.push(RhoPathStep::Node {
                        cursor: ParCursor::Par(value),
                        field: 0,
                    });
                    return Some(RhoPathEvent::MapKey(key));
                }
                RhoPathStep::Oneof(cursor) => match cursor.payload {
                    RhoOneofPayload::Scalar(value) => return Some(RhoPathEvent::Scalar(value)),
                    RhoOneofPayload::Message(cursor) => {
                        self.work.push(RhoPathStep::Node { cursor, field: 0 });
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod protobuf_resolution_tests {
    use super::*;
    use crate::rust::rholang::match_schema_tables::{
        EXPR_RHO_MATCH_PROGRAM, PAR_RHO_MATCH_PROGRAM,
    };

    static ONEOF_VARIANTS: &[RhoMatchOneofVariant] = &[
        RhoMatchOneofVariant {
            tag: 3,
            name: "boolean",
            payload: RhoMatchPayloadKind::Bool,
            protobuf_kind: ProtobufKind::Bool,
        },
        RhoMatchOneofVariant {
            tag: 7,
            name: "integer",
            payload: RhoMatchPayloadKind::I32,
            protobuf_kind: ProtobufKind::Int32,
        },
    ];
    static MATCH_FIELDS: &[RhoMatchField] = &[
        RhoMatchField {
            tag: 1,
            name: "items",
            kind: RhoMatchFieldKind::BytesSequence,
        },
        RhoMatchField {
            tag: 3,
            name: "choice",
            kind: RhoMatchFieldKind::Oneof(ONEOF_VARIANTS),
        },
    ];
    static PROTOBUF_FIELDS: &[ProtobufField] = &[
        ProtobufField {
            tag: 1,
            kind: ProtobufKind::RepeatedBytes,
            name: "items",
        },
        ProtobufField {
            tag: 3,
            kind: ProtobufKind::Oneof,
            name: "choice",
        },
    ];
    static PROGRAM: RhoMatchProgram = RhoMatchProgram {
        type_id: 999,
        type_name: "ResolutionFixture",
        fields: MATCH_FIELDS,
        protobuf_fields: PROTOBUF_FIELDS,
    };

    #[test]
    fn actual_tags_resolve_to_generated_slots_and_selected_oneof_payloads() {
        let repeated = resolve_protobuf_field(&PROGRAM, 1).expect("ordinary field resolves");
        assert_eq!(repeated.slot(), 0);
        assert_eq!(repeated.field().name, "items");
        assert_eq!(repeated.protobuf_kind(), ProtobufKind::RepeatedBytes);
        assert!(repeated.repeated());

        let oneof = resolve_protobuf_field(&PROGRAM, 7).expect("oneof arm resolves");
        assert_eq!(oneof.slot(), 1);
        assert_eq!(oneof.field().name, "choice");
        assert_eq!(oneof.protobuf_kind(), ProtobufKind::Int32);
        assert!(!oneof.repeated());
        assert!(matches!(
            oneof.payload(),
            ResolvedProtobufPayload::Oneof(variant) if variant.name == "integer"
        ));
        assert!(resolve_protobuf_field(&PROGRAM, 2).is_none());
    }

    #[test]
    fn absent_oneof_slot_retains_the_declared_marker_without_inventing_an_arm() {
        let oneof = protobuf_field_at_slot(&PROGRAM, 1).expect("oneof slot exists");
        assert_eq!(oneof.slot(), 1);
        assert_eq!(oneof.protobuf_kind(), ProtobufKind::Oneof);
        assert_eq!(
            oneof.payload(),
            ResolvedProtobufPayload::Declared(ProtobufKind::Oneof)
        );
        assert!(protobuf_field_at_slot(&PROGRAM, 2).is_none());
    }

    #[test]
    fn type_lookup_substitutes_raw_wire_par_and_preserves_generated_registry_rows() {
        let par = wire_program_by_type_id(PAR_RHO_MATCH_PROGRAM.type_id)
            .expect("Par type identifier resolves");
        assert!(std::ptr::eq(par, &PAR_PROTOBUF_REGION_PROGRAM));
        assert!(!par.protobuf_fields.is_empty());
        assert!(!std::ptr::eq(par, &PAR_RHO_MATCH_PROGRAM));

        let expr = wire_program_by_type_id(EXPR_RHO_MATCH_PROGRAM.type_id)
            .expect("generated Expr type identifier resolves");
        assert!(std::ptr::eq(expr, &EXPR_RHO_MATCH_PROGRAM));

        assert_eq!(
            wire_program_by_type_id(0),
            Err(WireProgramLookupError::ZeroTypeId)
        );
        assert_eq!(
            wire_program_by_type_id(u16::MAX),
            Err(WireProgramLookupError::OutsideRegistry)
        );
    }

    #[test]
    fn every_generated_wire_tag_resolves_to_its_exact_slot_and_payload() {
        let mut slot_count = 0;
        let mut arm_count = 0;
        let mut tag_count = 0;

        for (index, (name, resident)) in RHO_MATCH_REGISTRY.iter().enumerate() {
            assert_eq!(usize::from(resident.type_id), index + 1);
            let program = wire_program_by_type_id(resident.type_id)
                .expect("every registered type has a wire program");
            assert_eq!(program.type_name, *name);
            if *name == "Par" {
                assert!(std::ptr::eq(program, &PAR_PROTOBUF_REGION_PROGRAM));
                assert!(!std::ptr::eq(program, *resident));
            } else {
                assert!(std::ptr::eq(program, *resident));
            }
            assert_eq!(program.fields.len(), program.protobuf_fields.len());

            let mut tags = std::collections::BTreeSet::new();
            let mut previous_min_tag = 0;
            for (slot, (field, protobuf)) in program
                .fields
                .iter()
                .zip(program.protobuf_fields)
                .enumerate()
            {
                assert_eq!(field.tag, protobuf.tag, "{} slot {slot}", program.type_name);
                if program.type_name == "EPathMap" && field.tag == 9 {
                    assert_eq!(field.name, "entries");
                    assert_eq!(protobuf.name, "trie_snapshot");
                } else {
                    assert_eq!(
                        field.name, protobuf.name,
                        "{} slot {slot}",
                        program.type_name
                    );
                }
                assert!(
                    field.tag > previous_min_tag,
                    "{} slot {slot}",
                    program.type_name
                );
                previous_min_tag = field.tag;
                slot_count += 1;

                match field.kind {
                    RhoMatchFieldKind::Oneof(variants) => {
                        assert_eq!(protobuf.kind, ProtobufKind::Oneof);
                        assert!(!variants.is_empty());
                        assert_eq!(field.tag, variants.iter().map(|arm| arm.tag).min().unwrap());
                        for arm in variants {
                            assert!(arm.tag > 0 && arm.tag <= 0x1fff_ffff);
                            assert!(
                                tags.insert(arm.tag),
                                "{} tag {} collides",
                                program.type_name,
                                arm.tag
                            );
                            let resolved = resolve_protobuf_field(program, arm.tag)
                                .expect("declared oneof arm resolves");
                            assert_eq!(resolved.slot(), slot);
                            assert!(std::ptr::eq(resolved.field(), field));
                            assert_eq!(resolved.protobuf_kind(), arm.protobuf_kind);
                            assert!(resolved.protobuf_kind().wire_type().is_some());
                            assert!(!resolved.repeated());
                            assert!(matches!(
                                resolved.payload(),
                                ResolvedProtobufPayload::Oneof(actual)
                                    if std::ptr::eq(actual, arm)
                            ));
                            if let RhoMatchPayloadKind::Message(child_id) = arm.payload {
                                assert_eq!(arm.protobuf_kind, ProtobufKind::Message);
                                assert!(wire_program_by_type_id(child_id).is_ok());
                            }
                            arm_count += 1;
                        }
                    }
                    kind => {
                        assert!(protobuf.tag > 0 && protobuf.tag <= 0x1fff_ffff);
                        assert!(
                            tags.insert(protobuf.tag),
                            "{} tag {} collides",
                            program.type_name,
                            protobuf.tag
                        );
                        let resolved = resolve_protobuf_field(program, protobuf.tag)
                            .expect("declared field resolves");
                        assert_eq!(resolved.slot(), slot);
                        assert!(std::ptr::eq(resolved.field(), field));
                        assert_eq!(resolved.protobuf_kind(), protobuf.kind);
                        assert_eq!(
                            resolved.payload(),
                            ResolvedProtobufPayload::Declared(protobuf.kind)
                        );
                        assert_eq!(
                            resolved.repeated(),
                            matches!(
                                kind,
                                RhoMatchFieldKind::StringSequence
                                    | RhoMatchFieldKind::BytesSequence
                                    | RhoMatchFieldKind::MessageSequence(_)
                                    | RhoMatchFieldKind::StringMessageMap(_)
                            )
                        );
                        match kind {
                            RhoMatchFieldKind::OptionalMessage(child_id)
                            | RhoMatchFieldKind::MessageSequence(child_id)
                            | RhoMatchFieldKind::StringMessageMap(child_id) => {
                                assert!(wire_program_by_type_id(child_id).is_ok());
                            }
                            _ => {}
                        }
                    }
                }
            }

            let largest_tag = tags.last().copied().unwrap_or(0);
            for tag in 0..=largest_tag + 1 {
                assert_eq!(
                    resolve_protobuf_field(program, tag).is_some(),
                    tags.contains(&tag),
                    "{} tag {tag}",
                    program.type_name
                );
            }
            assert!(resolve_protobuf_field(program, u32::MAX).is_none());
            tag_count += tags.len();
        }

        assert_eq!(RHO_MATCH_REGISTRY.len(), 64);
        assert_eq!(slot_count, 165);
        assert_eq!(arm_count, 58);
        assert_eq!(tag_count, 217);
    }
}
