//! Differential, multiplicity, and fixed-stack gates for the shared NaN query.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EMap, ENot, ESet, ETuple, Expr, KeyValuePair, Par, Send};
use models::rust::par_bag::Multiplicity;
use models::rust::par_construction::{ParConstruction, PlannedPar};
use rho_pure_eval::par_contains_nan_double;

fn expr(instance: ExprInstance) -> Expr {
    Expr {
        expr_instance: Some(instance),
    }
}

fn par(instance: ExprInstance) -> Par { Par::default().with_exprs(vec![expr(instance)]) }

fn double_bits(bits: u64) -> Par { par(ExprInstance::GDouble(bits)) }

fn finite() -> Par { double_bits(1.5f64.to_bits()) }

fn nan() -> Par { double_bits(f64::NAN.to_bits()) }

fn list(ps: Vec<Par>) -> Par {
    par(ExprInstance::EListBody(EList {
        ps,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

fn tuple(ps: Vec<Par>) -> Par {
    par(ExprInstance::ETupleBody(ETuple {
        ps,
        locally_free: Vec::new(),
        connective_used: false,
    }))
}

fn set(ps: Vec<Par>) -> Par {
    par(ExprInstance::ESetBody(ESet {
        ps,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

fn map(kvs: Vec<KeyValuePair>) -> Par {
    par(ExprInstance::EMapBody(EMap {
        kvs,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

// Deliberately recursive and deliberately test-only: this is the bounded
// historical equation against which the production worklist is compared.
fn recursive_oracle(par: &Par) -> bool {
    par.decode_occurrences::<Expr>()
        .iter()
        .any(|expr| match &expr.expr_instance {
            Some(ExprInstance::GDouble(bits)) => f64::from_bits(*bits).is_nan(),
            Some(ExprInstance::EListBody(list)) => list.ps.iter().any(recursive_oracle),
            Some(ExprInstance::ETupleBody(tuple)) => tuple.ps.iter().any(recursive_oracle),
            Some(ExprInstance::ESetBody(set)) => set.ps.iter().any(recursive_oracle),
            Some(ExprInstance::EMapBody(map)) => map.kvs.iter().any(|pair| {
                pair.key.as_ref().is_some_and(recursive_oracle)
                    || pair.value.as_ref().is_some_and(recursive_oracle)
            }),
            _ => false,
        })
}

#[test]
fn direct_and_collection_semantics_match_the_historical_equation() {
    let alternate_nan = double_bits(0x7ff8_0000_dead_beef);
    let map_key = map(vec![KeyValuePair {
        key: Some(nan()),
        value: Some(finite()),
    }]);
    let map_value = map(vec![KeyValuePair {
        key: Some(finite()),
        value: Some(nan()),
    }]);
    let missing_map_arms = map(vec![
        KeyValuePair {
            key: None,
            value: Some(finite()),
        },
        KeyValuePair {
            key: Some(finite()),
            value: None,
        },
    ]);

    let cases = [
        ("empty", Par::default(), false),
        ("finite", finite(), false),
        (
            "positive infinity",
            double_bits(f64::INFINITY.to_bits()),
            false,
        ),
        (
            "negative infinity",
            double_bits(f64::NEG_INFINITY.to_bits()),
            false,
        ),
        ("canonical NaN", nan(), true),
        ("alternate NaN payload", alternate_nan, true),
        ("list", list(vec![finite(), nan()]), true),
        ("tuple", tuple(vec![list(vec![nan()])]), true),
        ("set", set(vec![tuple(vec![nan()])]), true),
        ("map key", map_key, true),
        ("map value", map_value, true),
        ("missing map arms", missing_map_arms, false),
        (
            "deep finite",
            list(vec![tuple(vec![set(vec![finite()])])]),
            false,
        ),
    ];

    for (name, value, expected) in cases {
        assert_eq!(
            recursive_oracle(&value),
            expected,
            "recursive control for {name}"
        );
        assert_eq!(
            par_contains_nan_double(&value),
            recursive_oracle(&value),
            "iterative parity for {name}"
        );
    }
}

#[test]
fn non_collection_children_and_process_families_remain_opaque() {
    let operator = par(ExprInstance::ENotBody(ENot { p: Some(nan()) }));
    let process = Par::default().with_sends(vec![Send {
        chan: Some(nan()),
        data: vec![nan()],
        persistent: false,
        locally_free: Vec::new(),
        connective_used: false,
        cost_region: None,
    }]);

    for (name, value) in [("operator", operator), ("process", process)] {
        assert!(!recursive_oracle(&value), "recursive control for {name}");
        assert!(
            !par_contains_nan_double(&value),
            "the shared query changed the historical boundary for {name}"
        );
    }
}

#[test]
fn compressed_multiplicity_is_never_expanded() {
    let maximum = Multiplicity::new(u64::MAX).expect("positive maximum multiplicity");
    for (name, instance, expected) in [
        ("NaN", ExprInstance::GDouble(f64::NAN.to_bits()), true),
        ("finite", ExprInstance::GDouble(1.5f64.to_bits()), false),
    ] {
        let mut value = Par::default();
        value
            .insert_component_many(&expr(instance), maximum, Vec::new(), false)
            .expect("one compressed endpoint stores maximum multiplicity");
        assert_eq!(value.family_distinct_len::<Expr>(), 1, "{name}");
        assert_eq!(value.family_cardinality::<Expr>(), u64::MAX, "{name}");
        assert_eq!(par_contains_nan_double(&value), expected, "{name}");
    }
}

fn import(arena: &mut ParConstruction, value: &Par) -> PlannedPar {
    arena.import_par(value).expect("import NaN-query terminal")
}

fn list_shell(children: usize) -> Expr {
    expr(ExprInstance::EListBody(EList {
        ps: vec![Par::default(); children],
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

fn wrap_list(arena: &mut ParConstruction, shell: &Expr, children: &[PlannedPar]) -> PlannedPar {
    let component = arena
        .expr_component(shell, children, false)
        .expect("plan EList component");
    arena
        .singleton_component(component, Vec::new(), false)
        .expect("plan singleton EList Par")
}

fn planned_nested_list(depth: usize, terminal: &Par) -> Par {
    let mut arena = ParConstruction::new();
    let shell = list_shell(1);
    let mut current = import(&mut arena, terminal);
    for _ in 0..depth {
        current = wrap_list(&mut arena, &shell, &[current]);
    }
    arena.finish(current).expect("finish one nested-list root")
}

fn planned_early_return_shape(depth: usize) -> Par {
    let mut arena = ParConstruction::new();
    let singleton_shell = list_shell(1);
    let mut deep_finite = import(&mut arena, &finite());
    for _ in 0..depth {
        deep_finite = wrap_list(&mut arena, &singleton_shell, &[deep_finite]);
    }
    let nan = import(&mut arena, &nan());
    // The machine extends children in source order and pops from the end. NaN
    // is therefore observed while the deep finite sibling remains pending.
    let root = wrap_list(&mut arena, &list_shell(2), &[deep_finite, nan]);
    arena.finish(root).expect("finish early-return root")
}

fn fixed_stack_probe(depth: usize, stack: usize) {
    std::thread::Builder::new()
        .name("nan-predicate-fixed-stack".to_string())
        .stack_size(stack)
        .spawn(move || {
            let started = std::time::Instant::now();
            let deep_nan = planned_nested_list(depth, &nan());
            let deep_finite = planned_nested_list(depth, &finite());
            let early_return = planned_early_return_shape(depth);
            eprintln!(
                "NaN predicate gate: constructed three depth-{depth} one-root plans in {:?}",
                started.elapsed()
            );

            let started = std::time::Instant::now();
            assert!(par_contains_nan_double(&deep_nan));
            eprintln!(
                "NaN predicate gate: found terminal NaN across {} collection levels in {:?}",
                depth,
                started.elapsed()
            );

            let started = std::time::Instant::now();
            assert!(!par_contains_nan_double(&deep_finite));
            eprintln!(
                "NaN predicate gate: exhausted finite depth-{depth} value in {:?}",
                started.elapsed()
            );

            let started = std::time::Instant::now();
            assert!(par_contains_nan_double(&early_return));
            eprintln!(
                "NaN predicate gate: early-returned with a depth-{depth} sibling pending in {:?}",
                started.elapsed()
            );

            // Teardown is part of the production composition. In particular,
            // the early-return call also dropped its still-pending deep work.
            let started = std::time::Instant::now();
            drop(deep_nan);
            drop(deep_finite);
            drop(early_return);
            eprintln!(
                "NaN predicate gate: dropped all three deep resident values in {:?}",
                started.elapsed()
            );
        })
        .expect("spawn fixed-stack NaN-query probe")
        .join()
        .expect("NaN query or ordinary teardown exhausted the fixed stack");
}

#[test]
fn moderate_query_and_early_teardown_fit_a_fixed_small_stack() {
    fixed_stack_probe(256, 256 * 1024);
}

#[test]
fn deep_query_and_early_teardown_fit_a_fixed_small_stack() { fixed_stack_probe(4_096, 256 * 1024); }

fn shallow_leaves() -> Vec<(String, Par)> {
    vec![
        ("empty".to_string(), Par::default()),
        ("finite".to_string(), finite()),
        ("nan".to_string(), nan()),
        (
            "alternate-nan".to_string(),
            double_bits(0x7ff8_0000_dead_beef),
        ),
        ("infinity".to_string(), double_bits(f64::INFINITY.to_bits())),
        (
            "not(nan)".to_string(),
            par(ExprInstance::ENotBody(ENot { p: Some(nan()) })),
        ),
    ]
}

fn entry(key: Option<Par>, value: Option<Par>) -> KeyValuePair { KeyValuePair { key, value } }

fn for_each_single_child_wrapper(children: &[(String, Par)], mut visit: impl FnMut(String, Par)) {
    for (a_name, a) in children {
        visit(format!("list[{a_name}]"), list(vec![a.clone()]));
        visit(format!("tuple[{a_name}]"), tuple(vec![a.clone()]));
        visit(format!("set[{a_name}]"), set(vec![a.clone()]));
        visit(
            format!("map[{a_name}: finite]"),
            map(vec![entry(Some(a.clone()), Some(finite()))]),
        );
        visit(
            format!("map[finite: {a_name}]"),
            map(vec![entry(Some(finite()), Some(a.clone()))]),
        );
        visit(
            format!("map[_: {a_name}]"),
            map(vec![entry(None, Some(a.clone()))]),
        );
    }
}

fn for_each_two_child_wrapper(
    left: &[(String, Par)],
    right: &[(String, Par)],
    mut visit: impl FnMut(String, Par),
) {
    for (a_name, a) in left {
        for (b_name, b) in right {
            visit(
                format!("list[{a_name}, {b_name}]"),
                list(vec![a.clone(), b.clone()]),
            );
            visit(
                format!("tuple[{a_name}, {b_name}]"),
                tuple(vec![a.clone(), b.clone()]),
            );
            visit(
                format!("set[{a_name}, {b_name}]"),
                set(vec![a.clone(), b.clone()]),
            );
            visit(
                format!("map[{a_name}: {b_name}]"),
                map(vec![entry(Some(a.clone()), Some(b.clone()))]),
            );
        }
    }
}

#[test]
fn every_generated_shallow_term_agrees_with_the_recursive_oracle() {
    let leaves = shallow_leaves();
    let siblings = [("finite".to_string(), finite()), ("nan".to_string(), nan())];
    let mut level_one = Vec::with_capacity(leaves.len() * 6 + leaves.len() * leaves.len() * 4);
    for_each_single_child_wrapper(&leaves, |name, value| level_one.push((name, value)));
    for_each_two_child_wrapper(&leaves, &leaves, |name, value| {
        level_one.push((name, value))
    });
    let mut children = Vec::with_capacity(leaves.len() + level_one.len());
    children.extend(leaves.iter().cloned());
    children.extend(level_one.iter().cloned());

    let mut checked = 0u64;
    let mut nan_positive = 0u64;
    let mut check = |name: &str, value: &Par| {
        let expected = recursive_oracle(value);
        assert_eq!(
            par_contains_nan_double(value),
            expected,
            "iterative parity for {name}"
        );
        checked += 1;
        nan_positive += u64::from(expected);
    };
    for (name, value) in &children {
        check(name, value);
    }
    for_each_single_child_wrapper(&children, |name, value| check(&name, &value));
    for_each_two_child_wrapper(&children, &siblings, |name, value| check(&name, &value));
    for_each_two_child_wrapper(&siblings, &children, |name, value| check(&name, &value));

    assert_eq!(checked, 4_278, "terms enumerated to depth two");
    assert_eq!(
        nan_positive, 2_898,
        "terms the recursive oracle reports as holding a NaN"
    );
}
