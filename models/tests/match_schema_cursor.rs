mod par_corpus;

#[test]
fn resident_expr_headers_match_every_generated_variant() {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EList, Par};
    use models::rust::canonical_path::takes_split_arm;
    use models::rust::rholang::protobuf_cursor::{CanonicalFieldCursor, CanonicalFieldValue};

    let corpus = par_corpus::every_expr_instance();
    let covered: BTreeSet<_> = corpus.iter().map(|(name, _)| *name).collect();
    let required: BTreeSet<_> = EXPR_INSTANCE_VARIANTS
        .iter()
        .map(|variant| variant.name)
        .collect();
    assert_eq!(covered, required);
    for (name, instance) in corpus.into_iter().chain([
        ("GBool", ExprInstance::GBool(false)),
        ("GInt", ExprInstance::GInt(0)),
        ("GString", ExprInstance::GString(String::new())),
        ("GUri", ExprInstance::GUri(String::new())),
        ("GByteArray", ExprInstance::GByteArray(Vec::new())),
        ("EListBody", ExprInstance::EListBody(EList::default())),
    ]) {
        let should_split = matches!(&instance, ExprInstance::EListBody(list)
            if list.remainder.is_none() && list.locally_free.is_empty() && !list.connective_used);
        let expr = Expr {
            expr_instance: Some(instance),
        };
        let schema = RhoPathCursor::new(ParCursor::from(&expr))
            .find_map(|event| match event {
                RhoPathEvent::EnterOneof(oneof) => Some(oneof),
                _ => None,
            })
            .unwrap();
        assert_eq!(schema.variant_name, name);
        let par = Par::default().with_exprs(vec![expr.clone()]);
        let mut endpoints = 0;
        par.visit_encoded::<Expr>(|entry| {
            endpoints += 1;
            assert_eq!(entry.multiplicity().get(), 1);
            let mut cursor = CanonicalFieldCursor::new(entry.payload());
            let field = cursor.next_field().unwrap().unwrap();
            assert_eq!(field.number, schema.protobuf_tag, "{name}");
            assert_eq!(field.number == 20, name == "EListBody", "{name}");
            if field.number == 20 {
                assert!(matches!(
                    field.value,
                    CanonicalFieldValue::LengthDelimited(_)
                ));
                assert!(entry.payload().starts_with(&[162, 1]));
            }
            assert!(
                cursor.next_field().unwrap().is_none(),
                "{name}: multiple oneof fields"
            );
        });
        assert_eq!(endpoints, 1);
        assert_eq!(takes_split_arm(&par), should_split, "{name}");
    }
    let absent = Par::default().with_exprs(vec![Expr::default()]);
    let mut endpoints = 0;
    absent.visit_encoded::<Expr>(|entry| {
        endpoints += 1;
        assert!(CanonicalFieldCursor::new(entry.payload())
            .next_field()
            .unwrap()
            .is_none());
    });
    assert_eq!(endpoints, 1);
    assert!(!takes_split_arm(&absent));
}

use std::collections::BTreeSet;

use models::rhoapi::{
    Bundle, Connective, CostTokenStack, DeployId, DeployerId, Expr, GUnforgeable, If, Match, New,
    PCost, Receive, Send,
};
use models::rust::rholang::bincode_schema_tables::{
    CONNECTIVE_INSTANCE_VARIANTS, EXPR_INSTANCE_VARIANTS, FORM_VARIANTS, TAGGED_CONT_VARIANTS,
    UNF_INSTANCE_VARIANTS, VAR_INSTANCE_VARIANTS,
};
use models::rust::rholang::match_schema::{
    RhoMatchFieldKind, RhoMatchPayloadKind, RhoPathCursor, RhoPathEvent,
};
use models::rust::rholang::match_schema_tables::{
    ParCursor, PAR_PROTOBUF_REGION_PROGRAM, RHO_MATCH_REGISTRY,
};
use models::rust::rholang::protobuf_schema::ProtobufKind;
use models::rust::rholang::schema_meta_tables::SCHEMA_CHILDREN;

fn field_kind_agrees_with_protobuf(
    match_kind: RhoMatchFieldKind,
    protobuf_kind: ProtobufKind,
) -> bool {
    matches!(
        (match_kind, protobuf_kind),
        (RhoMatchFieldKind::Bool, ProtobufKind::Bool)
            | (
                RhoMatchFieldKind::I32,
                ProtobufKind::Int32 | ProtobufKind::Sint32 | ProtobufKind::Sfixed32
            )
            | (
                RhoMatchFieldKind::U32,
                ProtobufKind::Uint32 | ProtobufKind::Fixed32
            )
            | (
                RhoMatchFieldKind::I64,
                ProtobufKind::Int64 | ProtobufKind::Sint64 | ProtobufKind::Sfixed64
            )
            | (
                RhoMatchFieldKind::U64,
                ProtobufKind::Uint64 | ProtobufKind::Fixed64
            )
            | (RhoMatchFieldKind::Bytes, ProtobufKind::Bytes)
            | (RhoMatchFieldKind::String, ProtobufKind::String)
            | (
                RhoMatchFieldKind::StringSequence,
                ProtobufKind::RepeatedString
            )
            | (
                RhoMatchFieldKind::BytesSequence,
                ProtobufKind::RepeatedBytes
            )
            | (RhoMatchFieldKind::OptionalMessage(_), ProtobufKind::Message)
            | (
                RhoMatchFieldKind::MessageSequence(_),
                ProtobufKind::RepeatedMessage
            )
            | (
                RhoMatchFieldKind::StringMessageMap(_),
                ProtobufKind::MapStringMessage
            )
            | (RhoMatchFieldKind::Oneof(_), ProtobufKind::Oneof)
            | (RhoMatchFieldKind::PathMap, ProtobufKind::EPathMapSnapshot)
    )
}

fn payload_kind_agrees_with_protobuf(
    match_kind: RhoMatchPayloadKind,
    protobuf_kind: ProtobufKind,
) -> bool {
    matches!(
        (match_kind, protobuf_kind),
        (RhoMatchPayloadKind::Bool, ProtobufKind::Bool)
            | (
                RhoMatchPayloadKind::I32,
                ProtobufKind::Int32 | ProtobufKind::Sint32 | ProtobufKind::Sfixed32
            )
            | (
                RhoMatchPayloadKind::U32,
                ProtobufKind::Uint32 | ProtobufKind::Fixed32
            )
            | (
                RhoMatchPayloadKind::I64,
                ProtobufKind::Int64 | ProtobufKind::Sint64 | ProtobufKind::Sfixed64
            )
            | (
                RhoMatchPayloadKind::U64,
                ProtobufKind::Uint64 | ProtobufKind::Fixed64
            )
            | (RhoMatchPayloadKind::Bytes, ProtobufKind::Bytes)
            | (RhoMatchPayloadKind::String, ProtobufKind::String)
            | (RhoMatchPayloadKind::Message(_), ProtobufKind::Message)
    )
}

fn record(
    cursor: ParCursor<'_>,
    types: &mut BTreeSet<&'static str>,
    variants: &mut BTreeSet<String>,
) {
    for event in RhoPathCursor::new(cursor) {
        match event {
            RhoPathEvent::EnterNode { program, .. } => {
                types.insert(program.type_name);
            }
            RhoPathEvent::EnterOneof(oneof) => {
                variants.insert(format!("{}::{}", oneof.oneof_name, oneof.variant_name));
            }
            _ => {}
        }
    }
}

#[test]
fn generated_program_registry_covers_the_descriptor_closure() {
    let mut type_ids = BTreeSet::new();
    let registry = RHO_MATCH_REGISTRY
        .iter()
        .enumerate()
        .map(|(index, (name, program))| {
            assert_eq!(*name, program.type_name);
            assert_eq!(
                usize::from(program.type_id),
                index + 1,
                "{name}: type identifiers must be one-based registry indices"
            );
            assert!(
                type_ids.insert(program.type_id),
                "{name}: duplicate type identifier {}",
                program.type_id
            );
            let mut previous = None;
            let mut names = BTreeSet::new();
            for field in program.fields {
                assert!(
                    previous.is_none_or(|previous| field.tag > previous),
                    "{name}: tags must be strictly ascending"
                );
                assert!(
                    names.insert(field.name),
                    "{name}: duplicate field {}",
                    field.name
                );
                let assert_type_id = |type_id: u16| {
                    let (_, child) = RHO_MATCH_REGISTRY
                        .get(usize::from(type_id) - 1)
                        .unwrap_or_else(|| {
                            panic!("{name}.{}: invalid child id {type_id}", field.name)
                        });
                    assert_eq!(child.type_id, type_id);
                };
                match field.kind {
                    RhoMatchFieldKind::OptionalMessage(type_id)
                    | RhoMatchFieldKind::MessageSequence(type_id)
                    | RhoMatchFieldKind::StringMessageMap(type_id) => assert_type_id(type_id),
                    RhoMatchFieldKind::Oneof(variants) => {
                        assert!(!variants.is_empty(), "{name}.{}: empty oneof", field.name);
                        assert_eq!(
                            field.tag,
                            variants.iter().map(|variant| variant.tag).min().unwrap(),
                            "{name}.{}: field tag must be the minimum arm tag",
                            field.name
                        );
                        let mut tags = BTreeSet::new();
                        for variant in variants {
                            assert!(tags.insert(variant.tag));
                            assert!(!variant.name.is_empty());
                            if let RhoMatchPayloadKind::Message(type_id) = variant.payload {
                                assert_type_id(type_id);
                            }
                        }
                    }
                    _ => {}
                }
                previous = Some(field.tag);
            }
            *name
        })
        .collect::<BTreeSet<_>>();
    let descriptor = SCHEMA_CHILDREN
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>();
    assert_eq!(registry, descriptor);
}

#[test]
fn generated_match_programs_share_the_protobuf_wire_schema() {
    for (name, program) in RHO_MATCH_REGISTRY {
        if *name == "Par" {
            assert!(
                program.protobuf_fields.is_empty(),
                "resident PathMap-backed Par must remain an opaque protobuf encoder cutover"
            );
            continue;
        }

        assert_eq!(
            program.fields.len(),
            program.protobuf_fields.len(),
            "{name}: structural and protobuf programs drifted"
        );
        for (match_field, protobuf_field) in program.fields.iter().zip(program.protobuf_fields) {
            assert_eq!(match_field.tag, protobuf_field.tag, "{name}: tag drift");
            if *name != "EPathMap" || match_field.tag != 9 {
                assert_eq!(match_field.name, protobuf_field.name, "{name}: name drift");
            } else {
                assert_eq!(match_field.name, "entries");
                assert_eq!(protobuf_field.name, "trie_snapshot");
            }
            assert!(
                field_kind_agrees_with_protobuf(match_field.kind, protobuf_field.kind),
                "{name}.{}: structural kind {:?} disagrees with protobuf kind {:?}",
                match_field.name,
                match_field.kind,
                protobuf_field.kind
            );
            if let RhoMatchFieldKind::Oneof(variants) = match_field.kind {
                let mut tags = BTreeSet::new();
                for variant in variants {
                    assert!(tags.insert(variant.tag), "{name}: duplicate oneof tag");
                    assert!(
                        payload_kind_agrees_with_protobuf(variant.payload, variant.protobuf_kind),
                        "{name}.{}: oneof payload {:?} disagrees with protobuf kind {:?}",
                        variant.name,
                        variant.payload,
                        variant.protobuf_kind
                    );
                    assert!(
                        variant.protobuf_kind.wire_type().is_some(),
                        "{name}.{}: concrete oneof arm must have a wire type",
                        variant.name
                    );
                }
            }
        }
    }
}

#[test]
fn external_par_has_a_distinct_descriptor_accurate_region_program() {
    let (_, resident) = RHO_MATCH_REGISTRY
        .iter()
        .find(|(name, _)| *name == "Par")
        .expect("Par must be in the generated registry");
    assert_eq!(PAR_PROTOBUF_REGION_PROGRAM.type_id, resident.type_id);
    assert_eq!(PAR_PROTOBUF_REGION_PROGRAM.type_name, resident.type_name);
    assert!(!PAR_PROTOBUF_REGION_PROGRAM.fields.is_empty());
    assert_eq!(
        PAR_PROTOBUF_REGION_PROGRAM.fields.len(),
        PAR_PROTOBUF_REGION_PROGRAM.protobuf_fields.len()
    );

    let mut previous = None;
    for (match_field, protobuf_field) in PAR_PROTOBUF_REGION_PROGRAM
        .fields
        .iter()
        .zip(PAR_PROTOBUF_REGION_PROGRAM.protobuf_fields)
    {
        assert!(previous.is_none_or(|tag| match_field.tag > tag));
        assert_eq!(match_field.tag, protobuf_field.tag);
        assert_eq!(match_field.name, protobuf_field.name);
        assert!(field_kind_agrees_with_protobuf(
            match_field.kind,
            protobuf_field.kind
        ));
        previous = Some(match_field.tag);
    }
}

#[test]
fn exhaustive_corpus_visits_every_message_and_oneof_variant() {
    let mut types = BTreeSet::new();
    let mut variants = BTreeSet::new();
    for (_, value) in par_corpus::par_corpus() {
        record(ParCursor::from(&value), &mut types, &mut variants);
    }
    for (_, value) in par_corpus::list_par_with_random_corpus() {
        record(ParCursor::from(&value), &mut types, &mut variants);
    }
    for (_, value) in par_corpus::bind_pattern_corpus() {
        record(ParCursor::from(&value), &mut types, &mut variants);
    }
    for (_, value) in par_corpus::tagged_continuation_corpus() {
        record(ParCursor::from(&value), &mut types, &mut variants);
    }
    for (_, value) in par_corpus::par_with_random_corpus() {
        record(ParCursor::from(&value), &mut types, &mut variants);
    }
    for (_, value) in par_corpus::list_bind_patterns_corpus() {
        record(ParCursor::from(&value), &mut types, &mut variants);
    }

    // `Par` is deliberately one native `ParBag` token in production. Walking
    // a Par must not decode its prefix-compressed endpoints back into the old
    // repeated-field tree merely to populate a search key. Exercise each
    // component family directly here so the generated descriptor programs
    // retain exhaustive test coverage without weakening that production
    // boundary.
    let all = par_corpus::all_par_fields();
    let cost = par_corpus::cost_annotated_par();
    macro_rules! record_components {
        ($source:expr, $component:ty) => {
            for value in $source.decode_occurrences::<$component>() {
                record(ParCursor::from(&value), &mut types, &mut variants);
            }
        };
    }
    record_components!(all, Send);
    record_components!(all, Receive);
    record_components!(all, New);
    record_components!(all, Expr);
    record_components!(all, Match);
    record_components!(all, GUnforgeable);
    record_components!(all, Bundle);
    record_components!(all, Connective);
    record_components!(all, If);
    record_components!(cost, Send);
    record_components!(cost, Receive);
    record_components!(cost, CostTokenStack);
    record(
        ParCursor::from(&par_corpus::cost_annotated_list_par_with_random()),
        &mut types,
        &mut variants,
    );
    record(
        ParCursor::from(&par_corpus::cost_annotated_tagged_continuation()),
        &mut types,
        &mut variants,
    );
    record(
        ParCursor::from(&PCost::default()),
        &mut types,
        &mut variants,
    );
    record(
        ParCursor::from(&DeployId::default()),
        &mut types,
        &mut variants,
    );
    record(
        ParCursor::from(&DeployerId::default()),
        &mut types,
        &mut variants,
    );

    let expected_types = RHO_MATCH_REGISTRY
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>();
    assert_eq!(types, expected_types);

    let mut expected_variants = BTreeSet::new();
    for (oneof, programs) in [
        ("ExprInstance", EXPR_INSTANCE_VARIANTS),
        ("ConnectiveInstance", CONNECTIVE_INSTANCE_VARIANTS),
        ("VarInstance", VAR_INSTANCE_VARIANTS),
        ("UnfInstance", UNF_INSTANCE_VARIANTS),
        ("TaggedCont", TAGGED_CONT_VARIANTS),
        ("Form", FORM_VARIANTS),
    ] {
        for program in programs {
            expected_variants.insert(format!("{oneof}::{}", program.name));
        }
    }
    assert_eq!(variants, expected_variants);
}

#[test]
fn epathmap_cursor_borrows_the_native_trie_without_forcing_a_snapshot() {
    let map = par_corpus::ground_pathmap();
    assert!(format!("{map:?}").contains("snapshot_cached: false"));
    let mut native = 0;
    for event in RhoPathCursor::new(ParCursor::from(&map)) {
        if let RhoPathEvent::PathMap(actual) = event {
            assert!(std::ptr::eq(actual, &map));
            native += 1;
        }
    }
    assert_eq!(native, 1);
    assert!(format!("{map:?}").contains("snapshot_cached: false"));
}

#[test]
fn deep_and_wide_cursor_walks_fit_a_small_native_stack() {
    std::thread::Builder::new()
        .name("rho-match-schema-cursor".to_string())
        .stack_size(256 * 1024)
        .spawn(|| {
            let deep = par_corpus::deep_par(20_000);
            let deep_events = RhoPathCursor::par(&deep).collect::<Vec<_>>();
            assert_eq!(
                deep_events
                    .iter()
                    .filter(|event| matches!(event, RhoPathEvent::EnterNode { .. }))
                    .count(),
                1,
                "native ParBag traversal must not decode nested Par endpoints"
            );
            assert_eq!(
                deep_events
                    .iter()
                    .filter(|event| matches!(event, RhoPathEvent::ParBag(_)))
                    .count(),
                1
            );

            let mut wide = par_corpus::all_par_fields();
            let mut exprs = wide.decode_occurrences::<models::rhoapi::Expr>();
            exprs.reserve(20_000);
            let seed = exprs[0].clone();
            exprs.extend(std::iter::repeat_n(seed, 20_000));
            wide.replace_exprs(exprs);
            let wide_events = RhoPathCursor::par(&wide).collect::<Vec<_>>();
            assert_eq!(
                wide_events
                    .iter()
                    .filter(|event| matches!(event, RhoPathEvent::EnterNode { .. }))
                    .count(),
                1
            );
            assert_eq!(
                wide_events
                    .iter()
                    .filter(|event| matches!(event, RhoPathEvent::ParBag(_)))
                    .count(),
                1
            );
        })
        .expect("spawn small-stack cursor worker")
        .join()
        .expect("small-stack cursor worker must complete");
}
