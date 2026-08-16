mod par_corpus;

use std::collections::BTreeSet;

use models::rhoapi::{DeployId, DeployerId, PCost};
use models::rust::rholang::bincode_schema_tables::{
    CONNECTIVE_INSTANCE_VARIANTS, EXPR_INSTANCE_VARIANTS, TAGGED_CONT_VARIANTS,
    UNF_INSTANCE_VARIANTS, VAR_INSTANCE_VARIANTS,
};
use models::rust::rholang::match_schema::{RhoPathCursor, RhoPathEvent};
use models::rust::rholang::match_schema_tables::{ParCursor, RHO_MATCH_REGISTRY};
use models::rust::rholang::schema_meta_tables::SCHEMA_CHILDREN;

fn record(
    cursor: ParCursor<'_>,
    types: &mut BTreeSet<&'static str>,
    variants: &mut BTreeSet<String>,
) {
    for event in RhoPathCursor::new(cursor) {
        match event {
            RhoPathEvent::EnterNode(program) => {
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
    let registry = RHO_MATCH_REGISTRY
        .iter()
        .map(|(name, program)| {
            assert_eq!(*name, program.type_name);
            let mut previous = 0;
            let mut names = BTreeSet::new();
            for field in program.fields {
                assert!(
                    field.tag > previous,
                    "{name}: tags must be strictly ascending"
                );
                assert!(
                    names.insert(field.name),
                    "{name}: duplicate field {}",
                    field.name
                );
                previous = field.tag;
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
            let deep_nodes = RhoPathCursor::par(&deep)
                .filter(|event| matches!(event, RhoPathEvent::EnterNode(_)))
                .count();
            assert!(deep_nodes > 20_000);

            let mut wide = par_corpus::all_par_fields();
            wide.exprs.reserve(20_000);
            let seed = wide.exprs[0].clone();
            wide.exprs.extend(std::iter::repeat_n(seed, 20_000));
            let wide_nodes = RhoPathCursor::par(&wide)
                .filter(|event| matches!(event, RhoPathEvent::EnterNode(_)))
                .count();
            assert!(wide_nodes > 20_000);
        })
        .expect("spawn small-stack cursor worker")
        .join()
        .expect("small-stack cursor worker must complete");
}
