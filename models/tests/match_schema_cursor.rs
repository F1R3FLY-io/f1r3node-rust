mod par_corpus;

use std::collections::BTreeSet;

use models::rhoapi::{
    Bundle, Connective, DeployId, DeployerId, Expr, GUnforgeable, If, Match, New, PCost, Receive,
    Send,
};
use models::rust::rholang::bincode_schema_tables::{
    CONNECTIVE_INSTANCE_VARIANTS, EXPR_INSTANCE_VARIANTS, TAGGED_CONT_VARIANTS,
    UNF_INSTANCE_VARIANTS, VAR_INSTANCE_VARIANTS,
};
use models::rust::rholang::match_schema::{
    RhoMatchFieldKind, RhoMatchPayloadKind, RhoPathCursor, RhoPathEvent,
};
use models::rust::rholang::match_schema_tables::{ParCursor, RHO_MATCH_REGISTRY};
use models::rust::rholang::schema_meta_tables::SCHEMA_CHILDREN;

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
    macro_rules! record_components {
        ($component:ty) => {
            for value in all.decode_occurrences::<$component>() {
                record(ParCursor::from(&value), &mut types, &mut variants);
            }
        };
    }
    record_components!(Send);
    record_components!(Receive);
    record_components!(New);
    record_components!(Expr);
    record_components!(Match);
    record_components!(GUnforgeable);
    record_components!(Bundle);
    record_components!(Connective);
    record_components!(If);
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
