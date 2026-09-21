use super::*;
use crate::rhoapi::{EMap, ESet, KeyValuePair, Var};
use proptest::prelude::*;
use std::cell::Cell;

#[path = "../par_corpus/mod.rs"]
mod corpus;

thread_local! {
    static IDENTITY_CURSORS: Cell<Option<usize>> = const { Cell::new(None) };
}

pub(super) fn record_identity_cursor() {
    IDENTITY_CURSORS.with(|counter| {
        if let Some(count) = counter.get() {
            counter.set(Some(count.checked_add(1).expect("identity cursor count")));
        }
    });
}

fn capture<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) { IDENTITY_CURSORS.with(|counter| counter.set(self.0)); }
    }
    let restore = Restore(IDENTITY_CURSORS.with(|counter| counter.replace(Some(0))));
    let value = operation();
    let count = IDENTITY_CURSORS.with(|counter| counter.get().unwrap());
    drop(restore);
    (value, count)
}

fn shell(is_map: bool, entries: &[(Par, Par)], lf: Vec<u8>, used: bool, remainder: Option<Var>) -> Expr {
    Expr { expr_instance: Some(if is_map {
        ExprInstance::EMapBody(EMap {
            kvs: entries.iter().map(|(key, value)| KeyValuePair {
                key: Some(key.clone()), value: Some(value.clone()),
            }).collect(),
            locally_free: lf, connective_used: used, remainder,
        })
    } else {
        ExprInstance::ESetBody(ESet {
            ps: entries.iter().map(|(key, _)| key.clone()).collect(),
            locally_free: lf, connective_used: used, remainder,
        })
    }) }
}

fn check_case(is_map: bool, entries: &[(Par, Par)], lf: Vec<u8>, used: bool, remainder: Option<Var>) -> usize {
    let expression = shell(is_map, entries, lf, used, remainder);
    let mut arena = ParConstruction::new();
    let mut children = Vec::new();
    for (key, value) in entries {
        children.push(arena.import_par(key).unwrap());
        if is_map { children.push(arena.import_par(value).unwrap()); }
    }
    let (component, visits) = capture(|| arena.expr_component(&expression, &children, false).unwrap());
    let expected_expr = ExprSortMatcher::sort_match(&expression).term;
    let oracle = arena.import_component(&expected_expr).unwrap();
    assert_eq!(arena.component_key(component).unwrap(), arena.component_key(oracle).unwrap());
    let planned = arena.par([(component, Multiplicity::ONE)], vec![3], true).unwrap();
    let expected = Par::default().with_exprs(vec![expected_expr]).with_locally_free(vec![3]).with_connective_used(true);
    assert_eq!(arena.protobuf_bytes(planned), expected.protobuf_bytes().unwrap());
    let actual = arena.finish(planned).unwrap();
    assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
    visits
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 96,
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/collection_collision_elision.proptest-regressions")
        ))),
        ..ProptestConfig::default()
    })]
    #[test]
    fn empty_singleton_observables_and_zero_identity_work(
        key in any::<i64>(), value in any::<i64>(), count in 0usize..=1,
        lf in proptest::collection::vec(any::<u8>(), 0..8), used in any::<bool>(),
        remainder in proptest::option::of(0i32..64),
    ) {
        let entries: Vec<_> = (0..count).map(|_| (corpus::gint(key), corpus::gint(value))).collect();
        let remainder = remainder.map(|index| Var { var_instance: Some(crate::rhoapi::var::VarInstance::FreeVar(index)) });
        for is_map in [false, true] {
            prop_assert_eq!(check_case(is_map, &entries, lf.clone(), used, remainder.clone()), 0);
        }
    }

    #[test]
    fn multi_entry_identity_work_and_observables_unchanged(
        entries in proptest::collection::vec((0i64..4, any::<i64>()), 2..8),
    ) {
        let entries: Vec<_> = entries.into_iter().enumerate().map(|(index, (key, value))|
            (corpus::gint(key).with_locally_free(vec![index as u8]), corpus::gint(value))).collect();
        for is_map in [false, true] {
            prop_assert_eq!(check_case(is_map, &entries, vec![5], true, None), entries.len());
        }
    }
}

#[test]
fn singleton_all_schema_families_preserve_score_and_bytes() {
    let cases = corpus::par_corpus();
    assert!(cases.len() >= 36);
    for (_, child) in cases {
        for is_map in [false, true] {
            assert_eq!(check_case(is_map, &[(child.clone(), child.clone())], vec![7], true, None), 0);
        }
    }
}

#[test]
fn nested_singletons_elide_positive_identity_work() {
    for is_map in [false, true] {
        let mut arena = ParConstruction::new();
        let mut child = arena.import_par(&corpus::gint(7)).unwrap();
        let value = arena.import_par(&corpus::gint(8)).unwrap();
        let expression = shell(is_map, &[(Par::default(), Par::default())], vec![], false, None);
        let (_, count) = capture(|| {
            for _ in 0..256 {
                let children = if is_map { vec![child, value] } else { vec![child] };
                let component = arena.expr_component(&expression, &children, false).unwrap();
                child = arena.par([(component, Multiplicity::ONE)], vec![], false).unwrap();
            }
        });
        assert_eq!(count, 0);
        let identity_len = WireByteCursor::new(&arena, WireRoot::ParSemanticIdentity(child)).count();
        assert!(identity_len > 256);
        let expression = shell(is_map, &vec![(Par::default(), Par::default()); 2], vec![], false, None);
        let children = if is_map { vec![child, value, child, value] } else { vec![child, child] };
        let (_, count) = capture(|| arena.expr_component(&expression, &children, false).unwrap());
        assert_eq!(count, 2);
    }
}

#[test]
fn arity_errors_precede_identity_work() {
    for is_map in [false, true] {
        let mut arena = ParConstruction::new();
        let child = arena.import_par(&corpus::gint(1)).unwrap();
        for count in 0..=2 {
            let entries = vec![(Par::default(), Par::default()); count];
            let expression = shell(is_map, &entries, vec![], false, None);
            let required = count * if is_map { 2 } else { 1 };
            for supplied in 0..=5 {
                if supplied == required { continue; }
                let (result, visits) = capture(|| arena.expr_component(&expression, &vec![child; supplied], false));
                assert!(result.is_err());
                assert_eq!(visits, 0);
            }
        }
    }
}

#[test]
fn identity_counter_is_nested_panic_safe_and_thread_local() {
    let (_, outer) = capture(|| {
        record_identity_cursor();
        let (_, inner) = capture(record_identity_cursor);
        assert_eq!(inner, 1);
        assert!(std::panic::catch_unwind(|| capture(|| panic!("scoped probe"))).is_err());
        std::thread::spawn(record_identity_cursor).join().unwrap();
        record_identity_cursor();
    });
    assert_eq!(outer, 2);
    assert_eq!(IDENTITY_CURSORS.with(Cell::get), None);
}
