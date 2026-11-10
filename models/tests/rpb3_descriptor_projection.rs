//! Exact, regenerable Rocq projection of the live RPB3 generated wire registry.
//!
//! The checked-in Rocq file is data, never the authority. Run the ignored
//! `emit_projection` test with `--ignored --nocapture`, copy only the text
//! between its markers, then run `projection_matches_live_registry`. That test
//! renders every live row and arm again and requires exact whole-file equality.

use std::collections::BTreeSet;
use std::fmt::Write;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Connective, CostRegion, GDeployId, GDeployerId, GPrivate, GSysAuthToken, GUnforgeable, Match,
    MatchCase, Par, Receive, ReceiveBind, Var,
};
use models::rust::par_bag::{with_component_key, ParBag, ParBagCodecError, PAR_BAG_KEY_DOMAIN};
use models::rust::rholang::canonical_semantics::{ParComponentTag, PAR_SCORE_TAGS};
use models::rust::rholang::match_schema::{
    RhoMatchFieldKind, RhoMatchPayloadKind, RhoMatchProgram,
};
use models::rust::rholang::match_schema_tables::{PAR_PROTOBUF_REGION_PROGRAM, RHO_MATCH_REGISTRY};
use models::rust::rholang::protobuf_encoder;
use models::rust::rholang::protobuf_schema::{ProtobufKind, ProtobufWireType};
use proptest::prelude::*;
use prost::Message as _;
use rspace_plus_plus::rspace::bag::Bag;

const CHECKED_IN: &str =
    include_str!("../../formal/rocq/stack_safe_pda/theories/RPB3DescriptorProjectionData.v");
const CHECKED_IN_VERUS: &str =
    include_str!("../../formal/verus/rpb3_generated_descriptor_projection.rs");

fn source_programs() -> Vec<(&'static RhoMatchProgram, bool)> {
    assert!(
        RHO_MATCH_REGISTRY.len() >= 60,
        "descriptor registry became vacuous"
    );
    for (index, (name, program)) in RHO_MATCH_REGISTRY.iter().enumerate() {
        assert_eq!(*name, program.type_name);
        assert_eq!(usize::from(program.type_id), index + 1);
    }
    assert_eq!(
        RHO_MATCH_REGISTRY
            .iter()
            .filter(|(name, _)| *name == "Par")
            .count(),
        1,
        "exactly one resident Par row must be substituted"
    );
    RHO_MATCH_REGISTRY
        .iter()
        .map(|(name, program)| {
            if *name == "Par" {
                assert_eq!(program.type_id, PAR_PROTOBUF_REGION_PROGRAM.type_id);
                assert_eq!(program.type_name, PAR_PROTOBUF_REGION_PROGRAM.type_name);
                assert!(program.protobuf_fields.is_empty());
                assert!(matches!(program.fields[0].kind, RhoMatchFieldKind::ParBag));
                (&PAR_PROTOBUF_REGION_PROGRAM, true)
            } else {
                (*program, false)
            }
        })
        .collect()
}

fn field_kind(kind: RhoMatchFieldKind) -> (&'static str, Option<u16>) {
    match kind {
        RhoMatchFieldKind::Bool => ("FKBool", None),
        RhoMatchFieldKind::I32 => ("FKI32", None),
        RhoMatchFieldKind::U32 => ("FKU32", None),
        RhoMatchFieldKind::I64 => ("FKI64", None),
        RhoMatchFieldKind::U64 => ("FKU64", None),
        RhoMatchFieldKind::Bytes => ("FKBytes", None),
        RhoMatchFieldKind::String => ("FKString", None),
        RhoMatchFieldKind::StringSequence => ("FKStringSequence", None),
        RhoMatchFieldKind::BytesSequence => ("FKBytesSequence", None),
        RhoMatchFieldKind::OptionalMessage(id) => ("FKOptionalMessage", Some(id)),
        RhoMatchFieldKind::MessageSequence(id) => ("FKMessageSequence", Some(id)),
        RhoMatchFieldKind::StringMessageMap(id) => ("FKStringMessageMap", Some(id)),
        RhoMatchFieldKind::Oneof(_) => ("FKOneof", None),
        RhoMatchFieldKind::ParBag => ("FKParBag", None),
        RhoMatchFieldKind::PathMap => ("FKPathMap", None),
    }
}

fn payload_kind(kind: RhoMatchPayloadKind) -> (&'static str, Option<u16>) {
    match kind {
        RhoMatchPayloadKind::Bool => ("PKBool", None),
        RhoMatchPayloadKind::I32 => ("PKI32", None),
        RhoMatchPayloadKind::U32 => ("PKU32", None),
        RhoMatchPayloadKind::I64 => ("PKI64", None),
        RhoMatchPayloadKind::U64 => ("PKU64", None),
        RhoMatchPayloadKind::Bytes => ("PKBytes", None),
        RhoMatchPayloadKind::String => ("PKString", None),
        RhoMatchPayloadKind::Message(id) => ("PKMessage", Some(id)),
    }
}

fn wire_kind(kind: Option<ProtobufWireType>) -> &'static str {
    match kind {
        None => "None",
        Some(ProtobufWireType::Varint) => "Some WTVarint",
        Some(ProtobufWireType::Fixed64) => "Some WTFixed64",
        Some(ProtobufWireType::LengthDelimited) => "Some WTLengthDelimited",
        Some(ProtobufWireType::Fixed32) => "Some WTFixed32",
    }
}

fn child_id(id: Option<u16>) -> String {
    match id {
        None => "None".to_owned(),
        Some(id) => format!("Some {id}"),
    }
}

fn default_policy(kind: RhoMatchFieldKind) -> &'static str {
    match kind {
        RhoMatchFieldKind::Bool
        | RhoMatchFieldKind::I32
        | RhoMatchFieldKind::U32
        | RhoMatchFieldKind::I64
        | RhoMatchFieldKind::U64
        | RhoMatchFieldKind::Bytes
        | RhoMatchFieldKind::String => "DefaultForbidden",
        RhoMatchFieldKind::StringSequence
        | RhoMatchFieldKind::BytesSequence
        | RhoMatchFieldKind::Oneof(_) => "DefaultAllowed",
        RhoMatchFieldKind::OptionalMessage(_)
        | RhoMatchFieldKind::MessageSequence(_)
        | RhoMatchFieldKind::StringMessageMap(_)
        | RhoMatchFieldKind::ParBag
        | RhoMatchFieldKind::PathMap => "DefaultNotScalar",
    }
}

fn repeated(kind: RhoMatchFieldKind) -> bool {
    matches!(
        kind,
        RhoMatchFieldKind::StringSequence
            | RhoMatchFieldKind::BytesSequence
            | RhoMatchFieldKind::MessageSequence(_)
            | RhoMatchFieldKind::StringMessageMap(_)
    )
}

fn rocq_string(value: &str) -> String {
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "generated descriptor name is outside the Rocq literal alphabet: {value:?}"
    );
    format!("\"{value}\"")
}

fn assert_field_wire_names(program: &RhoMatchProgram, field: &str, protobuf: &str, tag: u32) {
    if program.type_name == "EPathMap" && tag == 9 {
        assert_eq!(field, "entries");
        assert_eq!(protobuf, "trie_snapshot");
    } else {
        assert_eq!(field, protobuf);
    }
}

fn render_program(program: &RhoMatchProgram, raw_par: bool) -> String {
    assert_eq!(program.fields.len(), program.protobuf_fields.len());
    let mut slots = Vec::with_capacity(program.fields.len());
    let mut prior_tag = None;
    let mut seen_tags = BTreeSet::new();
    for (index, (field, protobuf)) in program
        .fields
        .iter()
        .zip(program.protobuf_fields)
        .enumerate()
    {
        assert_eq!(field.tag, protobuf.tag);
        assert_field_wire_names(program, field.name, protobuf.name, field.tag);
        assert!(prior_tag.is_none_or(|prior| prior < field.tag));
        prior_tag = Some(field.tag);
        let (kind, child) = field_kind(field.kind);
        let mut arms = Vec::new();
        if let RhoMatchFieldKind::Oneof(variants) = field.kind {
            assert!(!variants.is_empty());
            assert_eq!(field.tag, variants.iter().map(|arm| arm.tag).min().unwrap());
            for arm in variants {
                assert!(seen_tags.insert(arm.tag), "duplicate actual protobuf tag");
                let (payload, child) = payload_kind(arm.payload);
                assert!(arm.protobuf_kind.wire_type().is_some());
                arms.push(format!(
                    "{{| arm_tag := {}; arm_name := {}; arm_payload := {}; \
                     arm_protobuf_kind := PB{:?}; arm_wire := {}; arm_child_id := {} |}}",
                    arm.tag,
                    rocq_string(arm.name),
                    payload,
                    arm.protobuf_kind,
                    wire_kind(arm.protobuf_kind.wire_type()),
                    child_id(child),
                ));
            }
        } else {
            assert!(seen_tags.insert(field.tag), "duplicate actual protobuf tag");
        }
        let arms = format!("[{}]", arms.join("; "));
        slots.push(format!(
            "{{| slot_index := {index}; slot_min_tag := {}; slot_name := {}; \
             slot_protobuf_name := {}; \
             slot_field_kind := {kind}; slot_protobuf_kind := PB{:?}; \
             slot_wire := {}; slot_repeated := {}; slot_default := {}; \
             slot_child_id := {}; slot_arms := {arms} |}}",
            field.tag,
            rocq_string(field.name),
            rocq_string(protobuf.name),
            protobuf.kind,
            wire_kind(protobuf.kind.wire_type()),
            repeated(field.kind),
            default_policy(field.kind),
            child_id(child),
        ));
    }
    format!(
        "{{| program_type_id := {}; program_name := {}; program_is_raw_par := {}; \
         program_slots := [{}] |}}",
        program.type_id,
        rocq_string(program.type_name),
        raw_par,
        slots.join(";\n    "),
    )
}

fn render_native_program(program: &RhoMatchProgram) -> String {
    assert!(program.protobuf_fields.is_empty());
    let slots = program
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let (kind, child) = field_kind(field.kind);
            assert!(
                !matches!(field.kind, RhoMatchFieldKind::Oneof(_)),
                "native oneof requires an explicit arm projection"
            );
            format!(
                "{{| native_slot_index := {index}; native_slot_tag := {}; \
                 native_slot_name := {}; native_slot_kind := {kind}; \
                 native_slot_repeated := {}; native_slot_default := {}; \
                 native_slot_child_id := {} |}}",
                field.tag,
                rocq_string(field.name),
                repeated(field.kind),
                default_policy(field.kind),
                child_id(child),
            )
        })
        .collect::<Vec<_>>();
    format!(
        "{{| native_program_type_id := {}; native_program_name := {}; \
         native_program_slots := [{}]; native_program_protobuf_field_count := {} |}}",
        program.type_id,
        rocq_string(program.type_name),
        slots.join("; "),
        program.protobuf_fields.len(),
    )
}

fn render() -> String {
    let mut out = String::new();
    out.push_str(
        "(* @generated by models/tests/rpb3_descriptor_projection.rs from the live\n\
         RhoMatchProgram registry. DO NOT EDIT; regenerate with emit_projection. *)\n\
         From Stdlib Require Import List String.\n\
         Import ListNotations.\n\
         Open Scope string_scope.\n\
         Module GeneratedData.\n\
         Inductive field_kind :=\n\
           | FKBool | FKI32 | FKU32 | FKI64 | FKU64 | FKBytes | FKString\n\
           | FKStringSequence | FKBytesSequence | FKOptionalMessage\n\
           | FKMessageSequence | FKStringMessageMap | FKOneof | FKParBag | FKPathMap.\n\
         Inductive payload_kind :=\n\
           | PKBool | PKI32 | PKU32 | PKI64 | PKU64 | PKBytes | PKString | PKMessage.\n\
         Inductive protobuf_kind :=\n\
           | PBBool | PBInt32 | PBSint32 | PBSfixed32 | PBUint32 | PBFixed32\n\
           | PBInt64 | PBSint64 | PBSfixed64 | PBUint64 | PBFixed64\n\
           | PBFloat | PBDouble | PBString | PBBytes | PBRepeatedString\n\
           | PBRepeatedBytes | PBMessage | PBRepeatedMessage | PBMapStringMessage\n\
           | PBEPathMapSnapshot | PBOneof.\n\
         Inductive wire_kind := WTVarint | WTFixed64 | WTLengthDelimited | WTFixed32.\n\
         Inductive default_policy := DefaultForbidden | DefaultAllowed | DefaultNotScalar.\n\
         Record projection_arm := {\n\
           arm_tag : nat; arm_name : string; arm_payload : payload_kind;\n\
           arm_protobuf_kind : protobuf_kind; arm_wire : option wire_kind;\n\
           arm_child_id : option nat\n\
         }.\n\
         Record projection_slot := {\n\
           slot_index : nat; slot_min_tag : nat; slot_name : string;\n\
           slot_protobuf_name : string;\n\
           slot_field_kind : field_kind; slot_protobuf_kind : protobuf_kind;\n\
           slot_wire : option wire_kind; slot_repeated : bool;\n\
           slot_default : default_policy; slot_child_id : option nat;\n\
           slot_arms : list projection_arm\n\
         }.\n\
         Record projection_program := {\n\
           program_type_id : nat; program_name : string;\n\
           program_is_raw_par : bool; program_slots : list projection_slot\n\
         }.\n\
         Record native_slot := {\n\
           native_slot_index : nat; native_slot_tag : nat;\n\
           native_slot_name : string; native_slot_kind : field_kind;\n\
           native_slot_repeated : bool; native_slot_default : default_policy;\n\
           native_slot_child_id : option nat\n\
         }.\n\
         Record native_program := {\n\
           native_program_type_id : nat; native_program_name : string;\n\
           native_program_slots : list native_slot;\n\
           native_program_protobuf_field_count : nat\n\
         }.\n\n",
    );
    let programs = source_programs()
        .into_iter()
        .map(|(program, raw_par)| render_program(program, raw_par))
        .collect::<Vec<_>>();
    writeln!(
        out,
        "Definition generated_programs : list projection_program := ["
    )
    .unwrap();
    writeln!(out, "  {}", programs.join(";\n  ")).unwrap();
    writeln!(out, "].").unwrap();
    assert!(!PAR_PROTOBUF_REGION_PROGRAM.fields.is_empty());
    writeln!(
        out,
        "Definition raw_par_program : projection_program :=\n  {}.",
        render_program(&PAR_PROTOBUF_REGION_PROGRAM, true)
    )
    .unwrap();
    let resident_par = RHO_MATCH_REGISTRY
        .iter()
        .find(|(name, _)| *name == "Par")
        .expect("resident Par row is required")
        .1;
    writeln!(
        out,
        "Definition resident_par_program : native_program :=\n  {}.",
        render_native_program(resident_par)
    )
    .unwrap();
    out.push_str("End GeneratedData.\n");
    out
}

fn scalar_kind(kind: ProtobufKind) -> u8 {
    match kind {
        ProtobufKind::Bool => 1,
        ProtobufKind::Int32 => 2,
        ProtobufKind::Sint32 => 3,
        ProtobufKind::Uint32 => 4,
        ProtobufKind::Int64 => 5,
        ProtobufKind::Sint64 => 6,
        ProtobufKind::Uint64 => 7,
        ProtobufKind::Sfixed32 => 8,
        ProtobufKind::Fixed32 => 9,
        ProtobufKind::Float => 10,
        ProtobufKind::Sfixed64 => 11,
        ProtobufKind::Fixed64 => 12,
        ProtobufKind::Double => 13,
        ProtobufKind::String => 14,
        ProtobufKind::RepeatedString => 15,
        ProtobufKind::Bytes => 16,
        ProtobufKind::RepeatedBytes => 17,
        ProtobufKind::Message
        | ProtobufKind::RepeatedMessage
        | ProtobufKind::MapStringMessage
        | ProtobufKind::EPathMapSnapshot
        | ProtobufKind::Oneof => 0,
    }
}

fn verus_option_u16(value: Option<u16>) -> String {
    value.map_or_else(|| "None".to_owned(), |id| format!("Some({id})"))
}

fn verus_descriptor(
    slot: usize,
    repeated: bool,
    oneof_arm: Option<u32>,
    protobuf_kind: ProtobufKind,
    default_forbidden: bool,
    child_type_id: Option<u16>,
    frame_kind: u8,
) -> String {
    let scalar_kind = scalar_kind(protobuf_kind);
    assert_eq!(scalar_kind != 0, frame_kind == 0);
    assert!(!default_forbidden || scalar_kind != 0);
    assert_eq!(child_type_id.is_some(), frame_kind == 1 || frame_kind == 2);
    if let Some(id) = child_type_id {
        assert!(id != 0 && usize::from(id) <= RHO_MATCH_REGISTRY.len());
    }
    let oneof_arm = oneof_arm.map_or_else(|| "None".to_owned(), |tag| format!("Some({tag})"));
    let wire = protobuf_kind
        .wire_type()
        .expect("actual tag has a concrete wire kind") as u8;
    format!(
        "GeneratedDescriptor {{ slot: {slot}, repeated: {repeated}, oneof_arm: {oneof_arm}, \
         wire: {wire}, scalar_kind: {scalar_kind}, default_forbidden: {default_forbidden}, \
         child_type_id: {}, frame_kind: {frame_kind} }}",
        verus_option_u16(child_type_id),
    )
}

fn render_verus_program(program: &RhoMatchProgram, out: &mut String) {
    assert_eq!(program.fields.len(), program.protobuf_fields.len());
    writeln!(out, "        {} => match tag {{", program.type_id).unwrap();
    let mut seen_tags = std::collections::BTreeSet::new();
    let mut prior_min_tag = None;
    for (slot, (field, protobuf)) in program
        .fields
        .iter()
        .zip(program.protobuf_fields)
        .enumerate()
    {
        assert_eq!(field.tag, protobuf.tag);
        assert_field_wire_names(program, field.name, protobuf.name, field.tag);
        assert!(prior_min_tag.is_none_or(|prior| prior < field.tag));
        prior_min_tag = Some(field.tag);
        match field.kind {
            RhoMatchFieldKind::Oneof(arms) => {
                assert!(!arms.is_empty());
                assert_eq!(field.tag, arms.iter().map(|arm| arm.tag).min().unwrap());
                for arm in arms {
                    assert!(seen_tags.insert(arm.tag), "duplicate actual protobuf tag");
                    let (frame_kind, child_type_id) = match arm.payload {
                        RhoMatchPayloadKind::Message(id) => (1, Some(id)),
                        _ => (0, None),
                    };
                    let descriptor = verus_descriptor(
                        slot,
                        false,
                        Some(arm.tag),
                        arm.protobuf_kind,
                        false,
                        child_type_id,
                        frame_kind,
                    );
                    writeln!(out, "            {} => Some({descriptor}),", arm.tag).unwrap();
                }
            }
            kind => {
                assert!(seen_tags.insert(field.tag), "duplicate actual protobuf tag");
                let (frame_kind, child_type_id) = match kind {
                    RhoMatchFieldKind::OptionalMessage(id)
                    | RhoMatchFieldKind::MessageSequence(id) => (1, Some(id)),
                    RhoMatchFieldKind::StringMessageMap(id) => (2, Some(id)),
                    RhoMatchFieldKind::PathMap => (3, None),
                    RhoMatchFieldKind::ParBag | RhoMatchFieldKind::Oneof(_) => {
                        panic!("native or unresolved field entered a wire projection")
                    }
                    _ => (0, None),
                };
                let descriptor = verus_descriptor(
                    slot,
                    repeated(kind),
                    None,
                    protobuf.kind,
                    default_policy(kind) == "DefaultForbidden",
                    child_type_id,
                    frame_kind,
                );
                writeln!(out, "            {} => Some({descriptor}),", field.tag).unwrap();
            }
        }
    }
    writeln!(out, "            _ => None,").unwrap();
    writeln!(out, "        }},").unwrap();
}

fn render_verus() -> String {
    let programs = source_programs();
    let mut out = String::from(concat!(
        "use vstd::prelude::*;\n\nverus! {\n\n",
        "pub struct GeneratedDescriptor {\n",
        "    pub slot: usize,\n",
        "    pub repeated: bool,\n",
        "    pub oneof_arm: Option<u32>,\n",
        "    pub wire: u8,\n",
        "    pub scalar_kind: u8,\n",
        "    pub default_forbidden: bool,\n",
        "    pub child_type_id: Option<u16>,\n",
        "    pub frame_kind: u8,\n",
        "}\n\n",
        "pub open spec fn generated_slot_count(type_id: u16) -> Option<usize> {\n",
        "    match type_id {\n",
    ));
    for (program, _) in &programs {
        writeln!(
            out,
            "        {} => Some({}),",
            program.type_id,
            program.fields.len()
        )
        .unwrap();
    }
    out.push_str("        _ => None,\n    }\n}\n\n");
    out.push_str(concat!(
        "pub open spec fn generated_descriptor(type_id: u16, tag: u32) -> Option<GeneratedDescriptor> {\n",
        "    match type_id {\n",
    ));
    for (program, _) in &programs {
        render_verus_program(program, &mut out);
    }
    out.push_str("        _ => None,\n    }\n}\n\n}\n");
    out
}

#[test]
fn projection_matches_live_registry() {
    assert_eq!(
        CHECKED_IN,
        render(),
        "Rocq descriptor projection drifted; regenerate it from the live registry"
    );
}

#[test]
fn verus_projection_matches_live_registry() {
    assert_eq!(
        CHECKED_IN_VERUS,
        render_verus(),
        "Verus descriptor projection drifted; regenerate it from the live registry"
    );
}

fn source_score_integer(value: i64) -> Vec<u8> {
    let mut bytes = vec![1, 1];
    bytes.extend_from_slice(&((value as u64) ^ (1u64 << 63)).to_be_bytes());
    bytes
}

fn source_score_bytes(value: &[u8]) -> Vec<u8> {
    let mut bytes = vec![1, 3];
    for byte in value {
        bytes.push(*byte);
        if *byte == 0 {
            bytes.push(255);
        }
    }
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

fn source_score_constructor(
    constructor: i64,
    children: impl IntoIterator<Item = Vec<u8>>,
) -> Vec<u8> {
    let mut bytes = vec![2];
    bytes.extend(source_score_integer(constructor));
    for child in children {
        bytes.extend(child);
    }
    bytes.push(0);
    bytes
}

fn source_score_node(children: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    let mut bytes = vec![2];
    for child in children {
        bytes.extend(child);
    }
    bytes.push(0);
    bytes
}

enum SourceWireField<'a> {
    Scalar(u8, u8),
    Message(u8, &'a [u8]),
}

fn source_wire_fields(mut body: &[u8]) -> Option<Vec<SourceWireField<'_>>> {
    let mut fields = Vec::new();
    while let Some((&key, rest)) = body.split_first() {
        if key == 0 || key & 128 != 0 {
            return None;
        }
        let tag = key >> 3;
        match key & 7 {
            0 => {
                let (&value, tail) = rest.split_first()?;
                if value & 128 != 0 {
                    return None;
                }
                fields.push(SourceWireField::Scalar(tag, value));
                body = tail;
            }
            2 => {
                let (&length, tail) = rest.split_first()?;
                if length & 128 != 0 {
                    return None;
                }
                let length = usize::from(length);
                let payload = tail.get(..length)?;
                fields.push(SourceWireField::Message(tag, payload));
                body = tail.get(length..)?;
            }
            _ => return None,
        }
    }
    Some(fields)
}

fn source_small_par_score(body: &[u8]) -> Option<Vec<u8>> {
    let mut connective_used = false;
    let mut locally_free_seen = false;
    for field in source_wire_fields(body)? {
        match field {
            SourceWireField::Message(9, bytes) if !locally_free_seen && !bytes.is_empty() => {
                locally_free_seen = true;
            }
            SourceWireField::Scalar(10, 1) if !connective_used => {
                connective_used = true;
            }
            _ => return None,
        }
    }
    Some(source_score_constructor(999, [source_score_integer(
        if connective_used { 1 } else { 0 },
    )]))
}

fn source_u64_varint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    while value >= 128 {
        bytes.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}

fn source_signed_pbint32_bytes(value: i32) -> Vec<u8> { source_u64_varint(value as i64 as u64) }

fn source_decode_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().copied().take(10).enumerate() {
        if index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 127) << (index * 7);
        if byte & 128 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

fn source_signed_pbint32_field(mut body: &[u8], target_tag: u8) -> Option<i32> {
    let mut result = None;
    while let Some((&key, rest)) = body.split_first() {
        if key == 0 || key & 128 != 0 {
            return None;
        }
        let tag = key >> 3;
        match key & 7 {
            0 => {
                let (word, length) = source_decode_varint(rest)?;
                let encoded = rest.get(..length)?;
                if tag == target_tag {
                    let value = word as i32;
                    let canonical = source_signed_pbint32_bytes(value);
                    if result.is_some() || encoded != canonical.as_slice() {
                        return None;
                    }
                    result = Some(value);
                }
                body = rest.get(length..)?;
            }
            2 => {
                let (&length, tail) = rest.split_first()?;
                if length & 128 != 0 {
                    return None;
                }
                body = tail.get(usize::from(length)..)?;
            }
            _ => return None,
        }
    }
    result
}

fn source_empty_var_score(body: &[u8]) -> Option<Vec<u8>> {
    body.is_empty().then(|| source_score_integer(0))
}

fn source_empty_cost_region_score(body: &[u8]) -> Option<Vec<u8>> {
    if !body.is_empty() {
        return None;
    }
    let zero_coordinate = source_score_bytes(&[0; 8]);
    Some(source_score_constructor(500, [
        source_score_integer(0),
        zero_coordinate.clone(),
        zero_coordinate.clone(),
        zero_coordinate.clone(),
        zero_coordinate,
        source_score_constructor(506, []),
        source_score_integer(0),
    ]))
}

fn source_wire_message(tag: u8, payload: &[u8]) -> Vec<u8> {
    assert!(tag > 0 && tag < 16);
    assert!(payload.len() < 128);
    let mut body = vec![(tag << 3) | 2, payload.len() as u8];
    body.extend_from_slice(payload);
    body
}

fn source_receive_bind_score(body: &[u8]) -> Option<Vec<u8>> {
    let mut patterns = Vec::new();
    let mut source = None;
    let mut remainder = None;
    let mut cost = None;
    for field in source_wire_fields(body)? {
        match field {
            SourceWireField::Message(1, payload) => {
                patterns.push(source_small_par_score(payload)?);
            }
            SourceWireField::Message(2, payload) if source.is_none() => {
                source = Some(source_small_par_score(payload)?);
            }
            SourceWireField::Message(3, payload) if remainder.is_none() => {
                remainder = Some(source_empty_var_score(payload)?);
            }
            SourceWireField::Message(5, payload) if cost.is_none() => {
                cost = Some(source_empty_cost_region_score(payload)?);
            }
            _ => return None,
        }
    }
    let mut children = vec![source?];
    children.extend(patterns);
    children.push(remainder.unwrap_or(source_score_integer(0)));
    if let Some(cost) = cost {
        children.push(cost);
    }
    Some(source_score_node(children))
}

fn source_receive_score(body: &[u8]) -> Option<Vec<u8>> {
    let mut binds = Vec::new();
    let mut par_body = None;
    let mut persistent = 0;
    let mut peek = 0;
    let mut bind_count = 0;
    let mut connective_used = 0;
    let mut condition = None;
    for field in source_wire_fields(body)? {
        match field {
            SourceWireField::Message(1, payload) => {
                binds.push(source_receive_bind_score(payload)?);
            }
            SourceWireField::Message(2, payload) if par_body.is_none() => {
                par_body = Some(source_small_par_score(payload)?);
            }
            SourceWireField::Scalar(3, 1) => persistent = 1,
            SourceWireField::Scalar(4, 1) => peek = 1,
            SourceWireField::Scalar(5, value) if value > 0 => bind_count = value,
            SourceWireField::Scalar(7, 1) => connective_used = 1,
            SourceWireField::Message(8, payload) if condition.is_none() => {
                condition = Some(source_small_par_score(payload)?);
            }
            _ => return None,
        }
    }
    let mut children = vec![source_score_integer(persistent), source_score_integer(peek)];
    children.extend(binds);
    children.extend([
        par_body?,
        source_score_integer(i64::from(bind_count)),
        source_score_integer(connective_used),
        condition.unwrap_or(source_small_par_score(&[])?),
    ]);
    Some(source_score_constructor(301, children))
}

fn source_match_case_score(body: &[u8]) -> Option<Vec<u8>> {
    let mut pattern = None;
    let mut source = None;
    let mut free_count = 0;
    let mut guard = None;
    for field in source_wire_fields(body)? {
        match field {
            SourceWireField::Message(1, payload) if pattern.is_none() => {
                pattern = Some(source_small_par_score(payload)?);
            }
            SourceWireField::Message(2, payload) if source.is_none() => {
                source = Some(source_small_par_score(payload)?);
            }
            SourceWireField::Scalar(3, value) if value > 0 => free_count = value,
            SourceWireField::Message(4, payload) if guard.is_none() => {
                guard = Some(source_small_par_score(payload)?);
            }
            _ => return None,
        }
    }
    Some(source_score_node([
        pattern?,
        source?,
        source_score_integer(i64::from(free_count)),
        guard.unwrap_or(source_small_par_score(&[])?),
    ]))
}

fn source_match_score(body: &[u8]) -> Option<Vec<u8>> {
    let mut target = None;
    let mut cases = Vec::new();
    let mut connective_used = 0;
    for field in source_wire_fields(body)? {
        match field {
            SourceWireField::Message(1, payload) if target.is_none() => {
                target = Some(source_small_par_score(payload)?);
            }
            SourceWireField::Message(2, payload) => {
                cases.push(source_match_case_score(payload)?);
            }
            SourceWireField::Scalar(5, 1) => connective_used = 1,
            _ => return None,
        }
    }
    let mut children = vec![target?];
    children.extend(cases);
    children.push(source_score_integer(connective_used));
    Some(source_score_constructor(304, children))
}

fn assert_source_program(name: &str, type_id: u16, tags: &[u32]) {
    let (_, program) = RHO_MATCH_REGISTRY
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .expect("generated source program");
    assert_eq!(program.type_id, type_id);
    assert_eq!(
        program
            .fields
            .iter()
            .map(|field| field.tag)
            .collect::<Vec<_>>(),
        tags.to_vec()
    );
}

fn assert_source_message_child(name: &str, tag: u32, type_id: u16) {
    let (_, program) = RHO_MATCH_REGISTRY
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .expect("generated source program");
    let field = program
        .fields
        .iter()
        .find(|field| field.tag == tag)
        .expect("generated child field");
    assert!(matches!(
        field.kind,
        RhoMatchFieldKind::OptionalMessage(child_id)
            | RhoMatchFieldKind::MessageSequence(child_id) if child_id == type_id
    ));
}

fn source_connective_scalar_score(body: &[u8]) -> Option<Vec<u8>> {
    if body.is_empty() {
        return Some(source_score_integer(0));
    }
    let [key, value] = body else {
        return None;
    };
    let tag = *key >> 3;
    if *key & 7 != 0 || !(5..=9).contains(&tag) || *value > 1 {
        return None;
    }
    let mut score = vec![2];
    score.extend(source_score_integer(i64::from(tag) + 399));
    score.extend(source_score_integer(i64::from(*value)));
    score.push(0);
    Some(score)
}

fn source_component_key(family: ParComponentTag, body: &[u8], score: &[u8]) -> Vec<u8> {
    let mut key = PAR_BAG_KEY_DOMAIN.to_vec();
    key.push(family.byte());
    key.extend_from_slice(score);
    key.extend_from_slice(body);
    key.extend_from_slice(&(score.len() as u64).to_be_bytes());
    key
}

fn source_connective_key(body: &[u8], score: &[u8]) -> Vec<u8> {
    source_component_key(ParComponentTag::Connective, body, score)
}

fn admit_source_key(key: &[u8]) -> Result<ParBag, ParBagCodecError> {
    let mut raw = Bag::new();
    raw.insert(key).expect("the raw Bag accepts a byte path");
    ParBag::try_from_bag(raw)
}

fn connective_for_scalar_tag(tag: u8, value: bool) -> Connective {
    let instance = match tag {
        5 => ConnectiveInstance::ConnBool(value),
        6 => ConnectiveInstance::ConnInt(value),
        7 => ConnectiveInstance::ConnString(value),
        8 => ConnectiveInstance::ConnUri(value),
        9 => ConnectiveInstance::ConnByteArray(value),
        _ => unreachable!("the generated scalar arm has a tag from 5 through 9"),
    };
    Connective {
        connective_instance: Some(instance),
    }
}

fn source_unforgeable_body(tag: u8, payload: &[u8]) -> Vec<u8> {
    assert!((1..=4).contains(&tag));
    assert!(tag != 4 || payload.is_empty());
    assert!(payload.len() < 126);
    let mut nested = Vec::new();
    if tag != 4 && !payload.is_empty() {
        nested.push(10);
        nested.push(payload.len() as u8);
        nested.extend_from_slice(payload);
    }
    let mut body = vec![(tag << 3) | 2, nested.len() as u8];
    body.extend(nested);
    body
}

fn source_unforgeable_score(body: &[u8]) -> Option<Vec<u8>> {
    if body.is_empty() {
        return Some(source_score_constructor(0, []));
    }
    let [key, length, nested @ ..] = body else {
        return None;
    };
    if *key & 7 != 2 || *length & 128 != 0 || usize::from(*length) != nested.len() {
        return None;
    }
    match *key >> 3 {
        tag @ 1..=3 => {
            let payload = match nested {
                [] => &[][..],
                [10, length, bytes @ ..]
                    if *length > 0 && *length & 128 == 0 && usize::from(*length) == bytes.len() =>
                {
                    bytes
                }
                _ => return None,
            };
            let constructor = match tag {
                1 => 5,
                2 => 11,
                3 => 10,
                _ => unreachable!("three byte-bearing oneof arms"),
            };
            Some(source_score_constructor(constructor, [source_score_bytes(
                payload,
            )]))
        }
        4 if nested.is_empty() => Some(source_score_constructor(12, [])),
        _ => None,
    }
}

fn unforgeable_for_source_tag(tag: u8, payload: &[u8]) -> GUnforgeable {
    let instance = match tag {
        1 => UnfInstance::GPrivateBody(GPrivate {
            id: payload.to_vec(),
        }),
        2 => UnfInstance::GDeployIdBody(GDeployId {
            sig: payload.to_vec(),
        }),
        3 => UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: payload.to_vec(),
        }),
        4 => UnfInstance::GSysAuthTokenBody(GSysAuthToken {}),
        _ => unreachable!("the generated oneof arm has a tag from 1 through 4"),
    };
    GUnforgeable {
        unf_instance: Some(instance),
    }
}

#[test]
fn absent_connective_source_has_integer_zero_score() {
    let body: [u8; 0] = [];
    let score = source_connective_scalar_score(&body).expect("absent source score");
    assert_eq!(score, source_score_integer(0));
    let connective = Connective {
        connective_instance: None,
    };
    assert_eq!(protobuf_encoder::encode_to_vec(&connective), body.to_vec());
    assert_eq!(connective.encode_to_vec(), body.to_vec());
    let key = source_connective_key(&body, &score);
    assert_eq!(with_component_key(&connective, <[u8]>::to_vec), key);
    let admitted = admit_source_key(&key).expect("absent connective admits");
    assert_eq!(admitted.count::<Connective>(&connective), 1);
}

#[test]
fn absent_unforgeable_source_has_constructor_zero_score() {
    let body: [u8; 0] = [];
    let score = source_unforgeable_score(&body).expect("absent source score");
    assert_eq!(score, source_score_constructor(0, []));
    let unforgeable = GUnforgeable { unf_instance: None };
    assert_eq!(protobuf_encoder::encode_to_vec(&unforgeable), body.to_vec());
    assert_eq!(unforgeable.encode_to_vec(), body.to_vec());
    let key = source_component_key(ParComponentTag::GUnforgeable, &body, &score);
    assert_eq!(with_component_key(&unforgeable, <[u8]>::to_vec), key);
    let admitted = admit_source_key(&key).expect("absent unforgeable admits");
    assert_eq!(admitted.count::<GUnforgeable>(&unforgeable), 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_connective_scalar_score_matches_source_bytes_and_rejects_mutants(
        flags in any::<[bool; 5]>(),
        invalid_value in 2u8..=255u8,
    ) {
        let (_, program) = RHO_MATCH_REGISTRY.iter()
            .find(|(name, _)| *name == "Connective")
            .expect("generated Connective program");
        prop_assert_eq!(program.type_id, 55);
        let [field] = program.fields else {
            panic!("Connective has one generated oneof slot");
        };
        let RhoMatchFieldKind::Oneof(arms) = field.kind else {
            panic!("Connective has a generated oneof");
        };
        let scalar_tags = arms.iter()
            .filter(|arm| arm.payload == RhoMatchPayloadKind::Bool
                && arm.protobuf_kind == ProtobufKind::Bool)
            .map(|arm| arm.tag)
            .collect::<Vec<_>>();
        prop_assert_eq!(scalar_tags, vec![5, 6, 7, 8, 9]);

        for (offset, flag) in flags.into_iter().enumerate() {
            let tag = u8::try_from(offset + 5).expect("five scalar tags fit in u8");
            for value in [flag, !flag] {
                let body = [tag << 3, u8::from(value)];
                let score = source_connective_scalar_score(&body)
                    .expect("canonical source bool has an exact score");
                let connective = connective_for_scalar_tag(tag, value);
                prop_assert_eq!(protobuf_encoder::encode_to_vec(&connective), body.to_vec());
                prop_assert_eq!(connective.encode_to_vec(), body.to_vec());
                let key = source_connective_key(&body, &score);
                prop_assert_eq!(with_component_key(&connective, <[u8]>::to_vec), key.clone());
                let admitted = admit_source_key(&key)
                    .expect("canonical scalar connective admits");
                prop_assert_eq!(admitted.count::<Connective>(&connective), 1);

                let adjacent_tag = if tag == 9 { 5 } else { tag + 1 };
                let adjacent_body = [adjacent_tag << 3, u8::from(value)];
                let wrong_constructor = source_connective_scalar_score(&adjacent_body)
                    .expect("adjacent generated arm has an exact score");
                prop_assert_ne!(&score, &wrong_constructor);
                prop_assert!(admit_source_key(
                    &source_connective_key(&body, &wrong_constructor)).is_err());

                let flipped_body = [tag << 3, u8::from(!value)];
                let wrong_boolean = source_connective_scalar_score(&flipped_body)
                    .expect("opposite source bool has an exact score");
                prop_assert_ne!(&score, &wrong_boolean);
                prop_assert!(admit_source_key(
                    &source_connective_key(&body, &wrong_boolean)).is_err());

                for malformed in [
                    vec![body[0], 2],
                    vec![body[0], invalid_value],
                    vec![body[0], 0x80 | body[1], 0],
                    vec![body[0], body[1], body[0], body[1]],
                ] {
                    prop_assert!(source_connective_scalar_score(&malformed).is_none());
                    prop_assert!(admit_source_key(
                        &source_connective_key(&malformed, &score)).is_err());
                }
            }
        }
    }

    #[test]
    fn generated_connective_not_uses_nested_par_source_score_and_rejects_mutants(
        local_bit in 0u8..8u8,
    ) {
        let (_, program) = RHO_MATCH_REGISTRY.iter()
            .find(|(name, _)| *name == "Connective")
            .expect("generated Connective program");
        prop_assert_eq!(program.type_id, 55);
        prop_assert_eq!(PAR_PROTOBUF_REGION_PROGRAM.type_id, 1);
        let [field] = program.fields else {
            panic!("Connective has one generated oneof slot");
        };
        let RhoMatchFieldKind::Oneof(arms) = field.kind else {
            panic!("Connective has a generated oneof");
        };
        let not_arm = arms.iter().find(|arm| arm.tag == 3)
            .expect("generated ConnNotBody arm");
        prop_assert_eq!(not_arm.name, "ConnNotBody");
        prop_assert_eq!(not_arm.payload,
            RhoMatchPayloadKind::Message(PAR_PROTOBUF_REGION_PROGRAM.type_id));
        prop_assert_eq!(not_arm.protobuf_kind, ProtobufKind::Message);
        prop_assert_eq!(not_arm.protobuf_kind.wire_type(),
            Some(ProtobufWireType::LengthDelimited));

        for cached in [false, true] {
            for child_flag in [false, true] {
                let child = if cached {
                    Par::default().with_locally_free(vec![1u8 << local_bit])
                } else {
                    Par::default()
                }.with_connective_used(child_flag);
                let child_body = protobuf_encoder::encode_to_vec(&child);
                prop_assert_eq!(child.encode_to_vec(), child_body.clone());
                let child_score = source_small_par_score(&child_body)
                    .expect("canonical generated Par has a source score");
                let body = source_wire_message(3, &child_body);
                let connective = Connective {
                    connective_instance: Some(ConnectiveInstance::ConnNotBody(child.clone())),
                };
                prop_assert_eq!(protobuf_encoder::encode_to_vec(&connective), body.clone());
                prop_assert_eq!(connective.encode_to_vec(), body.clone());
                let decoded = Connective::decode(body.as_slice())
                    .expect("generated ConnNotBody decodes through Par");
                prop_assert_eq!(decoded.encode_to_vec(), body.clone());

                let score = source_score_constructor(400, [child_score]);
                let key = source_connective_key(&body, &score);
                prop_assert_eq!(with_component_key(&connective, <[u8]>::to_vec), key.clone());
                let admitted = admit_source_key(&key)
                    .expect("canonical generated ConnNotBody admits");
                prop_assert_eq!(admitted.count::<Connective>(&connective), 1);

                let other_child = child.clone().with_connective_used(!child_flag);
                let other_body = protobuf_encoder::encode_to_vec(&other_child);
                let other_score = source_score_constructor(400, [
                    source_small_par_score(&other_body).expect("opposite Par source score")
                ]);
                prop_assert_ne!(&score, &other_score);
                prop_assert!(admit_source_key(&source_connective_key(&body, &other_score)).is_err());

                let malformed_nested = source_wire_message(3, &[80, 0]);
                prop_assert!(source_small_par_score(&[80, 0]).is_none());
                prop_assert!(admit_source_key(
                    &source_connective_key(&malformed_nested, &score)).is_err());
                let wrong_wire = [24, 0];
                prop_assert!(admit_source_key(
                    &source_connective_key(&wrong_wire, &score)).is_err());
            }
        }
    }

    #[test]
    fn generated_par_score_contract_matches_live_families(
        registry_index in 0usize..RHO_MATCH_REGISTRY.len(),
    ) {
        for field in PAR_PROTOBUF_REGION_PROGRAM.fields {
            prop_assert!(!matches!(field.kind,
                RhoMatchFieldKind::OptionalMessage(_)
                | RhoMatchFieldKind::StringMessageMap(_)
                | RhoMatchFieldKind::Oneof(_)));
        }
        let children = PAR_PROTOBUF_REGION_PROGRAM.fields.iter().filter_map(|field| {
            match field.kind {
                RhoMatchFieldKind::MessageSequence(type_id) => Some((field.tag, type_id)),
                _ => None,
            }
        }).collect::<Vec<_>>();
        let family_tags = PAR_SCORE_TAGS.iter()
            .map(|family| u32::from(family.byte()))
            .collect::<BTreeSet<_>>();
        let child_tags = children.iter().map(|(tag, _)| *tag).collect::<BTreeSet<_>>();
        let child_type_ids = children.iter()
            .map(|(_, type_id)| *type_id)
            .collect::<BTreeSet<_>>();
        prop_assert_eq!(children.len(), PAR_SCORE_TAGS.len());
        prop_assert_eq!(family_tags.len(), PAR_SCORE_TAGS.len());
        prop_assert_eq!(child_tags, family_tags);
        prop_assert_eq!(child_type_ids.len(), PAR_SCORE_TAGS.len());
        for (_, type_id) in &children {
            prop_assert!(RHO_MATCH_REGISTRY.iter().any(|(_, program)| program.type_id == *type_id));
        }

        let (_, sampled) = RHO_MATCH_REGISTRY[registry_index];
        let required_by_score_families = PAR_SCORE_TAGS.iter().any(|family| {
            children.iter().any(|(tag, type_id)| {
                *tag == u32::from(family.byte()) && *type_id == sampled.type_id
            })
        });
        prop_assert_eq!(required_by_score_families, child_type_ids.contains(&sampled.type_id));

        let cost_tag = u32::from(ParComponentTag::CostTokenStack.byte());
        let cost_child = children.iter()
            .find(|(tag, _)| *tag == cost_tag)
            .expect("generated Par has a CostTokenStack child");
        prop_assert!(child_type_ids.contains(&cost_child.1));
        let cost_program = RHO_MATCH_REGISTRY.iter()
            .find(|(_, program)| program.type_id == cost_child.1)
            .expect("CostTokenStack child has a generated program");
        prop_assert_eq!(cost_program.0, "CostTokenStack");
        let without_cost = PAR_SCORE_TAGS.iter()
            .copied()
            .filter(|family| *family != ParComponentTag::CostTokenStack)
            .collect::<Vec<_>>();
        let mutant_requires_cost = without_cost.iter().any(|family| {
            children.iter().any(|(tag, type_id)| {
                *tag == u32::from(family.byte()) && *type_id == cost_child.1
            })
        });
        prop_assert!(!mutant_requires_cost);
        prop_assert_ne!(without_cost.len(), child_type_ids.len());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_locally_free_default_condition_and_guard_refuse_claimed_scores(
        local_bit in 0u8..8u8,
        body_flag in any::<bool>(),
        target_flag in any::<bool>(),
    ) {
        assert_source_message_child("Receive", 8, 1);
        assert_source_message_child("MatchCase", 4, 1);
        let locally_free = vec![1u8 << local_bit];
        let default_with_cache = Par::default().with_locally_free(locally_free.clone());
        let nondefault_with_cache = default_with_cache.clone().with_connective_used(true);
        let default_par_body = [74, 1, locally_free[0]];
        let nondefault_par_body = [74, 1, locally_free[0], 80, 1];
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&default_with_cache), default_par_body.to_vec());
        prop_assert_eq!(default_with_cache.encode_to_vec(), default_par_body.to_vec());
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&nondefault_with_cache), nondefault_par_body.to_vec());
        prop_assert_eq!(nondefault_with_cache.encode_to_vec(), nondefault_par_body.to_vec());
        prop_assert_eq!(source_small_par_score(&default_par_body), source_small_par_score(&[]));
        prop_assert_ne!(source_small_par_score(&nondefault_par_body), source_small_par_score(&[]));

        let receive = Receive {
            binds: vec![ReceiveBind {
                source: Some(Par::default()),
                ..ReceiveBind::default()
            }],
            body: Some(Par::default().with_connective_used(body_flag)),
            ..Receive::default()
        };
        let receive_body = protobuf_encoder::encode_to_vec(&receive);
        let receive_score = source_receive_score(&receive_body).unwrap();
        prop_assert_eq!(receive.encode_to_vec(), receive_body.clone());
        let receive_key = source_component_key(ParComponentTag::Receive, &receive_body, &receive_score);
        prop_assert_eq!(with_component_key(&receive, <[u8]>::to_vec), receive_key.clone());
        prop_assert!(admit_source_key(&receive_key).is_ok());
        let mut receive_default = receive.clone();
        receive_default.condition = Some(default_with_cache.clone());
        let receive_default_body = protobuf_encoder::encode_to_vec(&receive_default);
        prop_assert_eq!(receive_default.encode_to_vec(), receive_default_body.clone());
        prop_assert_eq!(source_receive_score(&receive_default_body), Some(receive_score.clone()));
        prop_assert!(admit_source_key(&source_component_key(
            ParComponentTag::Receive, &receive_default_body, &receive_score)).is_err());
        let mut receive_nondefault = receive;
        receive_nondefault.condition = Some(nondefault_with_cache.clone());
        let receive_nondefault_body = protobuf_encoder::encode_to_vec(&receive_nondefault);
        let receive_nondefault_score = source_receive_score(&receive_nondefault_body).unwrap();
        prop_assert_ne!(&receive_nondefault_score, &receive_score);
        prop_assert_eq!(receive_nondefault.encode_to_vec(), receive_nondefault_body.clone());
        let receive_nondefault_key = source_component_key(
            ParComponentTag::Receive, &receive_nondefault_body, &receive_nondefault_score);
        prop_assert_eq!(with_component_key(&receive_nondefault, <[u8]>::to_vec), receive_nondefault_key.clone());
        prop_assert!(admit_source_key(&receive_nondefault_key).is_ok());

        let value = Match {
            target: Some(Par::default().with_connective_used(target_flag)),
            cases: vec![MatchCase {
                pattern: Some(Par::default()),
                source: Some(Par::default()),
                ..MatchCase::default()
            }],
            ..Match::default()
        };
        let match_body = protobuf_encoder::encode_to_vec(&value);
        let match_score = source_match_score(&match_body).unwrap();
        prop_assert_eq!(value.encode_to_vec(), match_body.clone());
        let match_key = source_component_key(ParComponentTag::Match, &match_body, &match_score);
        prop_assert_eq!(with_component_key(&value, <[u8]>::to_vec), match_key.clone());
        prop_assert!(admit_source_key(&match_key).is_ok());
        let mut match_default = value.clone();
        match_default.cases[0].guard = Some(default_with_cache);
        let match_default_body = protobuf_encoder::encode_to_vec(&match_default);
        prop_assert_eq!(match_default.encode_to_vec(), match_default_body.clone());
        prop_assert_eq!(source_match_score(&match_default_body), Some(match_score.clone()));
        prop_assert!(admit_source_key(&source_component_key(
            ParComponentTag::Match, &match_default_body, &match_score)).is_err());
        let mut match_nondefault = value;
        match_nondefault.cases[0].guard = Some(nondefault_with_cache);
        let match_nondefault_body = protobuf_encoder::encode_to_vec(&match_nondefault);
        let match_nondefault_score = source_match_score(&match_nondefault_body).unwrap();
        prop_assert_ne!(&match_nondefault_score, &match_score);
        prop_assert_eq!(match_nondefault.encode_to_vec(), match_nondefault_body.clone());
        let match_nondefault_key = source_component_key(
            ParComponentTag::Match, &match_nondefault_body, &match_nondefault_score);
        prop_assert_eq!(with_component_key(&match_nondefault, <[u8]>::to_vec), match_nondefault_key.clone());
        prop_assert!(admit_source_key(&match_nondefault_key).is_ok());
    }

    #[test]
    fn generated_receive_and_match_case_signed_pbint32_scores_reject_unsigned_aliases(
        body_flag in any::<bool>(),
        target_flag in any::<bool>(),
    ) {
        assert_source_program("Receive", 19, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_source_program("MatchCase", 21, &[1, 2, 3, 4]);
        let body_par = Par::default().with_connective_used(body_flag);
        let target_par = Par::default().with_connective_used(target_flag);
        let body_par_wire = protobuf_encoder::encode_to_vec(&body_par);
        let target_par_wire = protobuf_encoder::encode_to_vec(&target_par);
        let body_par_score = source_small_par_score(&body_par_wire).unwrap();
        let target_par_score = source_small_par_score(&target_par_wire).unwrap();
        let empty_par_score = source_small_par_score(&[]).unwrap();
        for value in [-1, i32::MIN] {
            let receive = Receive {
                body: Some(body_par.clone()),
                bind_count: value,
                ..Receive::default()
            };
            let receive_body = protobuf_encoder::encode_to_vec(&receive);
            prop_assert_eq!(receive.encode_to_vec(), receive_body.clone());
            let decoded_count = source_signed_pbint32_field(&receive_body, 5)
                .expect("generated Receive has a canonical signed PBInt32");
            prop_assert_eq!(decoded_count, value);
            let receive_score = source_score_constructor(301, [
                source_score_integer(0),
                source_score_integer(0),
                body_par_score.clone(),
                source_score_integer(i64::from(decoded_count)),
                source_score_integer(0),
                empty_par_score.clone(),
            ]);
            let receive_key = source_component_key(ParComponentTag::Receive, &receive_body, &receive_score);
            prop_assert_eq!(with_component_key(&receive, <[u8]>::to_vec), receive_key.clone());
            prop_assert!(admit_source_key(&receive_key).is_ok());
            let forged_receive_score = source_score_constructor(301, [
                source_score_integer(0),
                source_score_integer(0),
                body_par_score.clone(),
                source_score_integer(i64::from(if value == -1 { i32::MIN } else { -1 })),
                source_score_integer(0),
                empty_par_score.clone(),
            ]);
            prop_assert_ne!(&forged_receive_score, &receive_score);
            prop_assert!(admit_source_key(&source_component_key(
                ParComponentTag::Receive, &receive_body, &forged_receive_score)).is_err());
            let unsigned_alias = source_u64_varint(u64::from(value as u32));
            prop_assert_eq!(unsigned_alias.len(), 5);
            prop_assert_eq!(source_signed_pbint32_bytes(value).len(), 10);
            let mut alias_receive_body = source_wire_message(2, &body_par_wire);
            alias_receive_body.push(40);
            alias_receive_body.extend_from_slice(&unsigned_alias);
            prop_assert!(source_signed_pbint32_field(&alias_receive_body, 5).is_none());
            prop_assert!(admit_source_key(&source_component_key(
                ParComponentTag::Receive, &alias_receive_body, &receive_score)).is_err());

            let value_message = Match {
                target: Some(target_par.clone()),
                cases: vec![MatchCase {
                    pattern: Some(Par::default()),
                    source: Some(Par::default()),
                    free_count: value,
                    ..MatchCase::default()
                }],
                ..Match::default()
            };
            let match_body = protobuf_encoder::encode_to_vec(&value_message);
            prop_assert_eq!(value_message.encode_to_vec(), match_body.clone());
            let match_fields = source_wire_fields(&match_body).unwrap();
            let [SourceWireField::Message(1, _), SourceWireField::Message(2, case_body)] =
                match_fields.as_slice() else {
                panic!("bounded Match has a target and one case");
            };
            let decoded_count = source_signed_pbint32_field(case_body, 3)
                .expect("generated MatchCase has a canonical signed PBInt32");
            prop_assert_eq!(decoded_count, value);
            let case_score = source_score_node([
                empty_par_score.clone(),
                empty_par_score.clone(),
                source_score_integer(i64::from(decoded_count)),
                empty_par_score.clone(),
            ]);
            let match_score = source_score_constructor(304, [
                target_par_score.clone(), case_score, source_score_integer(0),
            ]);
            let match_key = source_component_key(ParComponentTag::Match, &match_body, &match_score);
            prop_assert_eq!(with_component_key(&value_message, <[u8]>::to_vec), match_key.clone());
            prop_assert!(admit_source_key(&match_key).is_ok());
            let forged_case_score = source_score_node([
                empty_par_score.clone(),
                empty_par_score.clone(),
                source_score_integer(i64::from(if value == -1 { i32::MIN } else { -1 })),
                empty_par_score.clone(),
            ]);
            let forged_match_score = source_score_constructor(304, [
                target_par_score.clone(), forged_case_score, source_score_integer(0),
            ]);
            prop_assert_ne!(&forged_match_score, &match_score);
            prop_assert!(admit_source_key(&source_component_key(
                ParComponentTag::Match, &match_body, &forged_match_score)).is_err());
            let mut alias_case_body = source_wire_message(1, &[]);
            alias_case_body.extend(source_wire_message(2, &[]));
            alias_case_body.push(24);
            alias_case_body.extend_from_slice(&unsigned_alias);
            prop_assert!(source_signed_pbint32_field(&alias_case_body, 3).is_none());
            let mut alias_match_body = source_wire_message(1, &target_par_wire);
            alias_match_body.extend(source_wire_message(2, &alias_case_body));
            prop_assert!(admit_source_key(&source_component_key(
                ParComponentTag::Match, &alias_match_body, &match_score)).is_err());
        }
    }

    #[test]
    fn generated_receive_bind_child_shapes_and_types_refuse_claimed_score(
        pattern_flag in any::<bool>(),
        source_flag in any::<bool>(),
    ) {
        assert_source_message_child("Receive", 1, 16);
        assert_source_message_child("Receive", 2, 1);
        for (tag, type_id) in [(1, 1), (2, 1), (3, 6), (5, 12)] {
            assert_source_message_child("ReceiveBind", tag, type_id);
        }
        let pattern = Par::default().with_connective_used(pattern_flag);
        let source = Par::default().with_connective_used(source_flag);
        let pattern_body = if pattern_flag { &[80, 1][..] } else { &[][..] };
        let source_body = if source_flag { &[80, 1][..] } else { &[][..] };
        let bind_fields = [
            source_wire_message(1, pattern_body),
            source_wire_message(2, source_body),
            source_wire_message(3, &[]),
            source_wire_message(5, &[]),
        ];
        let bind_body = bind_fields.concat();
        let mut body = source_wire_message(1, &bind_body);
        body.extend(source_wire_message(2, &[]));
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: vec![pattern],
                source: Some(source),
                remainder: Some(Var::default()),
                cost_region: Some(CostRegion::default()),
                ..ReceiveBind::default()
            }],
            body: Some(Par::default()),
            ..Receive::default()
        };
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&receive), body.clone());
        prop_assert_eq!(receive.encode_to_vec(), body.clone());
        let score = source_receive_score(&body)
            .expect("all four typed ReceiveBind children have exact source scores");
        let key = source_component_key(ParComponentTag::Receive, &body, &score);
        prop_assert_eq!(with_component_key(&receive, <[u8]>::to_vec), key.clone());
        let admitted = admit_source_key(&key).expect("typed ReceiveBind children admit");
        prop_assert_eq!(admitted.count::<Receive>(&receive), 1);

        let foreign_var = [8, 2];
        let decoded_var = Var::decode(foreign_var.as_slice()).expect("foreign child is a valid Var");
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&decoded_var), foreign_var.to_vec());
        let foreign_par = [80, 1];
        let decoded_par = Par::decode(foreign_par.as_slice()).expect("foreign child is a valid Par");
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&decoded_par), foreign_par.to_vec());
        for (index, replacement) in [
            (1, vec![16, 1]),
            (1, source_wire_message(2, &[0])),
            (1, source_wire_message(2, &foreign_var)),
            (0, source_wire_message(1, &foreign_var)),
            (2, source_wire_message(3, &foreign_par)),
            (3, source_wire_message(5, &foreign_par)),
        ] {
            let mut fields = bind_fields.clone();
            fields[index] = replacement;
            let malformed_bind = fields.concat();
            let mut malformed_body = source_wire_message(1, &malformed_bind);
            malformed_body.extend(source_wire_message(2, &[]));
            prop_assert_ne!(&malformed_body, &body);
            prop_assert!(source_receive_score(&malformed_body).is_none());
            prop_assert!(admit_source_key(&source_component_key(
                ParComponentTag::Receive, &malformed_body, &score)).is_err());
        }
    }

    #[test]
    fn generated_match_case_child_shapes_and_types_refuse_claimed_score(
        pattern_flag in any::<bool>(),
        source_flag in any::<bool>(),
        free_count in 1u8..64u8,
    ) {
        assert_source_message_child("Match", 1, 1);
        assert_source_message_child("Match", 2, 21);
        for tag in [1, 2, 4] {
            assert_source_message_child("MatchCase", tag, 1);
        }
        let pattern = Par::default().with_connective_used(pattern_flag);
        let source = Par::default().with_connective_used(source_flag);
        let guard = Par::default().with_connective_used(true);
        let pattern_body = if pattern_flag { &[80, 1][..] } else { &[][..] };
        let source_body = if source_flag { &[80, 1][..] } else { &[][..] };
        let case_fields = [
            source_wire_message(1, pattern_body),
            source_wire_message(2, source_body),
            vec![24, free_count],
            source_wire_message(4, &[80, 1]),
        ];
        let case_body = case_fields.concat();
        let mut body = source_wire_message(1, &[]);
        body.extend(source_wire_message(2, &case_body));
        let value = Match {
            target: Some(Par::default()),
            cases: vec![MatchCase {
                pattern: Some(pattern),
                source: Some(source),
                free_count: i32::from(free_count),
                guard: Some(guard),
                ..MatchCase::default()
            }],
            ..Match::default()
        };
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&value), body.clone());
        prop_assert_eq!(value.encode_to_vec(), body.clone());
        let score = source_match_score(&body)
            .expect("all required MatchCase children have exact source scores");
        let key = source_component_key(ParComponentTag::Match, &body, &score);
        prop_assert_eq!(with_component_key(&value, <[u8]>::to_vec), key.clone());
        let admitted = admit_source_key(&key).expect("typed MatchCase children admit");
        prop_assert_eq!(admitted.count::<Match>(&value), 1);

        let foreign_var = [8, 2];
        let decoded_var = Var::decode(foreign_var.as_slice()).expect("foreign child is a valid Var");
        prop_assert_eq!(protobuf_encoder::encode_to_vec(&decoded_var), foreign_var.to_vec());
        for (index, replacement) in [
            (0, vec![8, 1]),
            (0, source_wire_message(1, &[0])),
            (0, source_wire_message(1, &foreign_var)),
            (1, vec![16, 1]),
            (1, source_wire_message(2, &[0])),
            (1, source_wire_message(2, &foreign_var)),
            (3, vec![32, 1]),
            (3, source_wire_message(4, &[0])),
            (3, source_wire_message(4, &foreign_var)),
        ] {
            let mut fields = case_fields.clone();
            fields[index] = replacement;
            let malformed_case = fields.concat();
            let mut malformed_body = source_wire_message(1, &[]);
            malformed_body.extend(source_wire_message(2, &malformed_case));
            prop_assert_ne!(&malformed_body, &body);
            prop_assert!(source_match_score(&malformed_body).is_none());
            prop_assert!(admit_source_key(&source_component_key(
                ParComponentTag::Match, &malformed_body, &score)).is_err());
        }
    }

    #[test]
    fn generated_receive_bind_score_matches_nested_source_and_rejects_mutants(
        source_flag in any::<bool>(),
        body_flag in any::<bool>(),
        pattern_order in any::<bool>(),
        persistent in any::<bool>(),
        peek in any::<bool>(),
        connective_used in any::<bool>(),
        has_condition in any::<bool>(),
    ) {
        assert_source_program("ReceiveBind", 16, &[1, 2, 3, 4, 5]);
        assert_source_program("Receive", 19, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let empty = Par::default();
        let marked = Par::default().with_connective_used(true);
        let patterns = if pattern_order {
            vec![empty.clone(), marked.clone()]
        } else {
            vec![marked.clone(), empty.clone()]
        };
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns,
                source: Some(Par::default().with_connective_used(source_flag)),
                ..ReceiveBind::default()
            }],
            body: Some(Par::default().with_connective_used(body_flag)),
            persistent,
            peek,
            bind_count: 1,
            connective_used,
            condition: has_condition.then_some(marked),
            ..Receive::default()
        };
        let body = protobuf_encoder::encode_to_vec(&receive);
        prop_assert_eq!(receive.encode_to_vec(), body.clone());
        let score = source_receive_score(&body)
            .expect("bounded canonical Receive source has an exact score");
        let key = source_component_key(ParComponentTag::Receive, &body, &score);
        prop_assert_eq!(with_component_key(&receive, <[u8]>::to_vec), key.clone());
        let admitted = admit_source_key(&key).expect("complete Receive source admits");
        prop_assert_eq!(admitted.count::<Receive>(&receive), 1);

        let mut missing_bind_source = receive.clone();
        missing_bind_source.binds[0].source = None;
        let mut missing_body = receive.clone();
        missing_body.body = None;
        for incomplete in [missing_bind_source, missing_body] {
            let incomplete_body = protobuf_encoder::encode_to_vec(&incomplete);
            prop_assert_eq!(incomplete.encode_to_vec(), incomplete_body.clone());
            prop_assert!(source_receive_score(&incomplete_body).is_none());
            prop_assert!(admit_source_key(&source_component_key(
                ParComponentTag::Receive, &incomplete_body, &score)).is_err());
        }

        let mut reversed = receive.clone();
        reversed.binds[0].patterns.reverse();
        let reversed_score = source_receive_score(&protobuf_encoder::encode_to_vec(&reversed))
            .expect("reversed patterns have an exact source score");
        prop_assert_ne!(&score, &reversed_score);
        prop_assert!(admit_source_key(&source_component_key(
            ParComponentTag::Receive, &body, &reversed_score)).is_err());

        let mut changed_source = receive.clone();
        changed_source.binds[0].source.as_mut().unwrap().connective_used = !source_flag;
        let changed_source_score = source_receive_score(
            &protobuf_encoder::encode_to_vec(&changed_source))
            .expect("changed bind source has an exact score");
        prop_assert_ne!(&score, &changed_source_score);
        prop_assert!(admit_source_key(&source_component_key(
            ParComponentTag::Receive, &body, &changed_source_score)).is_err());

        let mut changed_scalar = receive.clone();
        changed_scalar.persistent = !persistent;
        let changed_scalar_score = source_receive_score(
            &protobuf_encoder::encode_to_vec(&changed_scalar))
            .expect("changed Receive scalar has an exact score");
        prop_assert_ne!(&score, &changed_scalar_score);
        prop_assert!(admit_source_key(&source_component_key(
            ParComponentTag::Receive, &body, &changed_scalar_score)).is_err());

        let mut absent_condition = receive.clone();
        absent_condition.condition = None;
        let absent_body = protobuf_encoder::encode_to_vec(&absent_condition);
        let absent_score = source_receive_score(&absent_body).unwrap();
        let mut present_default = absent_condition;
        present_default.condition = Some(empty);
        let default_body = protobuf_encoder::encode_to_vec(&present_default);
        prop_assert_eq!(present_default.encode_to_vec(), default_body.clone());
        prop_assert_eq!(source_receive_score(&default_body), Some(absent_score.clone()));
        prop_assert!(admit_source_key(&source_component_key(
            ParComponentTag::Receive, &default_body, &absent_score)).is_err());
    }

    #[test]
    fn generated_match_case_score_matches_nested_source_and_rejects_mutants(
        target_flag in any::<bool>(),
        pattern_flag in any::<bool>(),
        source_flag in any::<bool>(),
        connective_used in any::<bool>(),
        has_guard in any::<bool>(),
        free_count in 1i32..64i32,
    ) {
        assert_source_program("MatchCase", 21, &[1, 2, 3, 4]);
        assert_source_program("Match", 22, &[1, 2, 4, 5]);
        let empty = Par::default();
        let marked = Par::default().with_connective_used(true);
        let value = Match {
            target: Some(Par::default().with_connective_used(target_flag)),
            cases: vec![MatchCase {
                pattern: Some(Par::default().with_connective_used(pattern_flag)),
                source: Some(Par::default().with_connective_used(source_flag)),
                free_count,
                guard: has_guard.then_some(marked),
                ..MatchCase::default()
            }],
            connective_used,
            ..Match::default()
        };
        let body = protobuf_encoder::encode_to_vec(&value);
        prop_assert_eq!(value.encode_to_vec(), body.clone());
        let score = source_match_score(&body)
            .expect("bounded canonical Match source has an exact score");
        let key = source_component_key(ParComponentTag::Match, &body, &score);
        prop_assert_eq!(with_component_key(&value, <[u8]>::to_vec), key.clone());
        let admitted = admit_source_key(&key).expect("complete Match source admits");
        prop_assert_eq!(admitted.count::<Match>(&value), 1);

        let mut missing_target = value.clone();
        missing_target.target = None;
        let mut missing_pattern = value.clone();
        missing_pattern.cases[0].pattern = None;
        let mut missing_source = value.clone();
        missing_source.cases[0].source = None;
        for incomplete in [missing_target, missing_pattern, missing_source] {
            let incomplete_body = protobuf_encoder::encode_to_vec(&incomplete);
            prop_assert_eq!(incomplete.encode_to_vec(), incomplete_body.clone());
            prop_assert!(source_match_score(&incomplete_body).is_none());
            prop_assert!(admit_source_key(&source_component_key(
                ParComponentTag::Match, &incomplete_body, &score)).is_err());
        }

        let mut changed_target = value.clone();
        changed_target.target.as_mut().unwrap().connective_used = !target_flag;
        let changed_target_score = source_match_score(
            &protobuf_encoder::encode_to_vec(&changed_target))
            .expect("changed Match target has an exact source score");
        prop_assert_ne!(&score, &changed_target_score);
        prop_assert!(admit_source_key(&source_component_key(
            ParComponentTag::Match, &body, &changed_target_score)).is_err());

        let mut changed_pattern = value.clone();
        changed_pattern.cases[0].pattern.as_mut().unwrap().connective_used = !pattern_flag;
        let changed_pattern_score = source_match_score(
            &protobuf_encoder::encode_to_vec(&changed_pattern))
            .expect("changed case pattern has an exact source score");
        prop_assert_ne!(&score, &changed_pattern_score);
        prop_assert!(admit_source_key(&source_component_key(
            ParComponentTag::Match, &body, &changed_pattern_score)).is_err());

        let mut changed_scalar = value.clone();
        changed_scalar.cases[0].free_count += 1;
        let changed_scalar_score = source_match_score(
            &protobuf_encoder::encode_to_vec(&changed_scalar))
            .expect("changed case scalar has an exact source score");
        prop_assert_ne!(&score, &changed_scalar_score);
        prop_assert!(admit_source_key(&source_component_key(
            ParComponentTag::Match, &body, &changed_scalar_score)).is_err());

        let mut absent_guard = value.clone();
        absent_guard.cases[0].guard = None;
        let absent_body = protobuf_encoder::encode_to_vec(&absent_guard);
        let absent_score = source_match_score(&absent_body).unwrap();
        let mut present_default = absent_guard;
        present_default.cases[0].guard = Some(empty);
        let default_body = protobuf_encoder::encode_to_vec(&present_default);
        prop_assert_eq!(present_default.encode_to_vec(), default_body.clone());
        prop_assert_eq!(source_match_score(&default_body), Some(absent_score.clone()));
        prop_assert!(admit_source_key(&source_component_key(
            ParComponentTag::Match, &default_body, &absent_score)).is_err());
    }

    #[test]
    fn generated_unforgeable_scores_match_nested_source_and_reject_mutants(
        payload in prop::collection::vec(any::<u8>(), 0..24),
    ) {
        let (_, program) = RHO_MATCH_REGISTRY.iter()
            .find(|(name, _)| *name == "GUnforgeable")
            .expect("generated GUnforgeable program");
        prop_assert_eq!(program.type_id, 60);
        let [field] = program.fields else {
            panic!("GUnforgeable has one generated oneof slot");
        };
        let RhoMatchFieldKind::Oneof(arms) = field.kind else {
            panic!("GUnforgeable has a generated oneof");
        };
        let child_ids = arms.iter().map(|arm| {
            assert_eq!(arm.protobuf_kind, ProtobufKind::Message);
            let RhoMatchPayloadKind::Message(child_id) = arm.payload else {
                panic!("every unforgeable arm has a message child");
            };
            (arm.tag, child_id)
        }).collect::<Vec<_>>();
        prop_assert_eq!(child_ids, vec![(1, 61), (2, 62), (3, 63), (4, 64)]);
        for child_id in [61, 62, 63] {
            let (_, child) = RHO_MATCH_REGISTRY.iter()
                .find(|(_, candidate)| candidate.type_id == child_id)
                .expect("generated byte-bearing unforgeable child");
            let [slot] = child.fields else {
                panic!("unforgeable byte child has one generated slot");
            };
            prop_assert_eq!(slot.tag, 1);
            prop_assert_eq!(slot.kind, RhoMatchFieldKind::Bytes);
            prop_assert_eq!(child.protobuf_fields[0].kind, ProtobufKind::Bytes);
        }
        let (_, sys_auth) = RHO_MATCH_REGISTRY.iter()
            .find(|(_, candidate)| candidate.type_id == 64)
            .expect("generated system-authority child");
        prop_assert!(sys_auth.fields.is_empty());

        for tag in 1u8..=4u8 {
            let values = if tag == 4 {
                vec![Vec::new()]
            } else {
                vec![payload.clone(), Vec::new(), vec![0, 255]]
            };
            for value in values {
                let body = source_unforgeable_body(tag, &value);
                let score = source_unforgeable_score(&body)
                    .expect("canonical generated source has an exact score");
                let unforgeable = unforgeable_for_source_tag(tag, &value);
                prop_assert_eq!(protobuf_encoder::encode_to_vec(&unforgeable), body.clone());
                prop_assert_eq!(unforgeable.encode_to_vec(), body.clone());
                let key = source_component_key(ParComponentTag::GUnforgeable, &body, &score);
                prop_assert_eq!(with_component_key(&unforgeable, <[u8]>::to_vec), key.clone());
                let admitted = admit_source_key(&key)
                    .expect("canonical unforgeable source admits");
                prop_assert_eq!(admitted.count::<GUnforgeable>(&unforgeable), 1);

                let other_tag = match tag { 1 => 2, 2 => 3, 3 => 1, 4 => 1, _ => unreachable!() };
                let other_body = source_unforgeable_body(
                    other_tag,
                    if tag == 4 { &[] } else { value.as_slice() },
                );
                let wrong_constructor = source_unforgeable_score(&other_body)
                    .expect("other generated arm has an exact score");
                prop_assert_ne!(&score, &wrong_constructor);
                prop_assert!(admit_source_key(&source_component_key(
                    ParComponentTag::GUnforgeable, &body, &wrong_constructor)).is_err());

                if tag != 4 {
                    let mut changed = value.clone();
                    changed.push(1);
                    let changed_body = source_unforgeable_body(tag, &changed);
                    let wrong_payload = source_unforgeable_score(&changed_body)
                        .expect("changed nested bytes have an exact score");
                    prop_assert_ne!(&score, &wrong_payload);
                    prop_assert!(admit_source_key(&source_component_key(
                        ParComponentTag::GUnforgeable, &body, &wrong_payload)).is_err());
                }

                let mut wrong_length = body.clone();
                wrong_length[1] += 1;
                let duplicate = [body.as_slice(), body.as_slice()].concat();
                let explicit_default = vec![(tag << 3) | 2, 2, 10, 0];
                for malformed in [wrong_length, duplicate, explicit_default] {
                    prop_assert!(source_unforgeable_score(&malformed).is_none());
                    prop_assert!(admit_source_key(&source_component_key(
                        ParComponentTag::GUnforgeable, &malformed, &score)).is_err());
                }
            }
        }
    }
}

#[test]
#[ignore = "prints the full reproducible Rocq artifact for deliberate regeneration"]
fn emit_projection() {
    println!("BEGIN RPB3 DESCRIPTOR PROJECTION");
    print!("{}", render());
    println!("END RPB3 DESCRIPTOR PROJECTION");
}

#[test]
#[ignore = "prints the full reproducible Verus artifact for deliberate regeneration"]
fn emit_verus_projection() {
    println!("BEGIN RPB3 VERUS DESCRIPTOR PROJECTION");
    print!("{}", render_verus());
    println!("END RPB3 VERUS DESCRIPTOR PROJECTION");
}
