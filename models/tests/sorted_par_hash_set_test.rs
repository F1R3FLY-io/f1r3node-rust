use std::collections::HashSet;

use models::create_bit_vector;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{ESet, GPrivate, GUnforgeable, Par};
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::sorted_par_hash_set::SortedParHashSet;
use models::rust::utils::{
    new_boundvar_par, new_emethod_expr, new_eplus_par, new_gbool_par, new_gint_expr, new_gint_par,
};
use prost::Message;

fn create_pars() -> Vec<Par> {
    let par_ground = Par::default().with_exprs(vec![
        new_gint_expr(2),
        new_gint_expr(1),
        new_gint_expr(-1),
        new_gint_expr(-2),
        new_gint_expr(0),
    ]);

    let par_expr = new_eplus_par(
        new_eplus_par(
            new_gint_par(1, Vec::new(), false),
            new_gint_par(3, Vec::new(), false),
        ),
        new_gint_par(2, Vec::new(), false),
    );

    let par_methods = Par::default().with_exprs(vec![
        new_emethod_expr(
            "nth".to_string(),
            new_boundvar_par(2, create_bit_vector(&[2]), false),
            vec![new_gint_par(1, Vec::new(), false)],
            create_bit_vector(&[2]),
        ),
        new_emethod_expr(
            "nth".to_string(),
            new_boundvar_par(2, create_bit_vector(&[2]), false),
            vec![new_gint_par(1, Vec::new(), false)],
            create_bit_vector(&[2]),
        ),
        new_emethod_expr(
            "nth".to_string(),
            new_boundvar_par(2, create_bit_vector(&[2]), false),
            vec![
                new_gint_par(2, Vec::new(), false),
                new_gint_par(3, Vec::new(), false),
            ],
            create_bit_vector(&[2]),
        ),
        new_emethod_expr(
            "nth".to_string(),
            new_boundvar_par(2, create_bit_vector(&[2]), false),
            vec![new_gint_par(2, Vec::new(), false)],
            create_bit_vector(&[2]),
        ),
    ]);

    vec![par_ground, par_expr, par_methods]
}

fn sample() -> SortedParHashSet {
    let pars = create_pars();
    SortedParHashSet::create_from_vec(pars)
}

fn roundtrip_test(par_tree_set: &SortedParHashSet) {
    let serialized = serialize_eset(par_tree_set);
    let deserialized = ESet::decode(&*serialized).expect("Failed to deserialize ESet");
    assert_eq!(
        deserialized.ps, par_tree_set.sorted_pars,
        "Roundtrip serialization failed: deserialized ps does not match sorted_pars"
    );
}

fn serialize_eset(par_tree_set: &SortedParHashSet) -> Vec<u8> {
    let eset = ESet {
        ps: par_tree_set.sorted_pars.clone(),
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    };
    eset.encode_to_vec()
}

#[test]
fn sorted_par_hash_set_should_preserve_structure_during_round_trip_protobuf_serialization() {
    let par_tree_set = sample();
    roundtrip_test(&par_tree_set);
}

#[test]
fn sorted_par_hash_set_should_preserve_ordering_during_serialization_required_for_deterministic_serialization(
) {
    let reference_bytes = serialize_eset(&sample());

    // Ensure serialization is deterministic across 1000 iterations
    for i in 1..=1000 {
        let serialized = serialize_eset(&sample());
        let res = serialized == reference_bytes; // Equivalent to util.Arrays.equals from Scala
        assert!(
            res,
            "Run #{} serialization failed: serialized bytes differ from reference bytes",
            i
        );
    }
}

#[test]
fn sorted_par_hash_set_should_deduplicate_its_elements_where_last_seen_element_wins() {
    fn deduplicate(in_seq: Vec<Par>) -> HashSet<Par> { in_seq.into_iter().collect() }

    let elements: Vec<Par> = vec![
        new_gint_par(1, Vec::new(), false),
        new_gint_par(1, Vec::new(), false),
        new_gbool_par(true, Vec::new(), false),
        new_gbool_par(true, Vec::new(), false),
        new_gbool_par(false, Vec::new(), false),
        Par::default().with_exprs(vec![new_emethod_expr(
            "nth".to_string(),
            new_boundvar_par(2, create_bit_vector(&[2]), false),
            vec![new_gint_par(1, Vec::new(), false)],
            create_bit_vector(&[2]),
        )]),
        Par::default().with_exprs(vec![new_emethod_expr(
            "nth".to_string(),
            new_boundvar_par(2, create_bit_vector(&[2]), false),
            vec![new_gint_par(1, Vec::new(), false)],
            create_bit_vector(&[2]),
        )]),
    ];

    let expected = deduplicate(elements.clone());

    let shs = SortedParHashSet::create_from_vec(elements);

    assert_eq!(
        shs.sorted_pars.into_iter().collect::<HashSet<_>>(),
        expected,
        "SortedParHashSet did not deduplicate elements correctly"
    );
}

#[test]
fn sorted_par_hash_set_should_be_equal_when_it_is_equal() {
    let elements: Vec<Par> = vec![Par {
        unforgeables: vec![
            GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![0] })),
            },
            GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate::default())),
            },
        ],
        connective_used: true,
        ..Default::default()
    }];

    assert_eq!(
        SortedParHashSet::create_from_vec(elements.clone()),
        SortedParHashSet::create_from_vec(elements),
        "SortedParHashSet instances created from the same elements should be equal"
    );
}

#[test]
fn sorted_par_hash_set_should_sort_all_input() {
    let pars = create_pars();
    let set = SortedParHashSet::create_from_vec(pars.clone());

    if let Some(unsorted) = pars.first() {
        let sorted = ParSortMatcher::sort_match(unsorted).term;

        check_sorted_input(
            |elem: Par| inserted(&set, &elem),
            unsorted.clone(),
            sorted.clone(),
        );
        check_sorted_input(
            |elem: Par| removed(&set, &elem),
            unsorted.clone(),
            sorted.clone(),
        );
        let eset = ESet {
            ps: pars.clone(),
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        };
        check_sorted_input(
            |elem: Par| ParSetTypeMapper::contains(&eset, &elem),
            unsorted.clone(),
            sorted.clone(),
        );
    }
}

#[test]
fn sorted_par_hash_set_should_preserve_sortedness_of_all_elements_and_whole_map() {
    let pars = create_pars();
    let set = SortedParHashSet::create_from_vec(pars.clone());

    check_sorted(&set.sorted_pars);

    if let Some(par) = pars.first() {
        check_sorted(&inserted(&set, par));
        check_sorted(&removed(&set, par));
        let other = SortedParHashSet::create_from_vec(vec![par.clone()]).sorted_pars;
        check_sorted(&ParSetTypeMapper::union(set.sorted_pars.clone(), other));
    }
}

#[test]
fn canonical_set_operations_match_a_rebuilt_set() {
    let pars = create_pars();
    let set = SortedParHashSet::create_from_vec(pars[..2].to_vec()).sorted_pars;
    let other = SortedParHashSet::create_from_vec(pars[1..].to_vec()).sorted_pars;
    let rebuilt = |ps: Vec<Par>| SortedParHashSet::create_from_vec(ps).sorted_pars;

    assert_eq!(inserted_ps(&set, &pars[2]), rebuilt(pars.clone()));
    assert_eq!(inserted_ps(&set, &pars[0]), set);
    assert_eq!(removed_ps(&set, &pars[0]), rebuilt(vec![pars[1].clone()]));
    assert_eq!(removed_ps(&set, &pars[2]), set);
    assert_eq!(
        ParSetTypeMapper::union(set.clone(), other.clone()),
        rebuilt(pars.clone())
    );
    assert_eq!(
        ParSetTypeMapper::diff(set.clone(), &other),
        rebuilt(vec![pars[0].clone()])
    );
    assert_eq!(ParSetTypeMapper::diff(set.clone(), &[]), set);
}

fn inserted(set: &SortedParHashSet, elem: &Par) -> Vec<Par> { inserted_ps(&set.sorted_pars, elem) }

fn removed(set: &SortedParHashSet, elem: &Par) -> Vec<Par> { removed_ps(&set.sorted_pars, elem) }

fn inserted_ps(ps: &[Par], elem: &Par) -> Vec<Par> {
    let mut ps = ps.to_vec();
    ParSetTypeMapper::insert(&mut ps, elem);
    ps
}

fn removed_ps(ps: &[Par], elem: &Par) -> Vec<Par> {
    let mut ps = ps.to_vec();
    ParSetTypeMapper::remove(&mut ps, elem);
    ps
}

fn check_sorted(iterable: &[Par]) {
    for par in iterable {
        assert!(is_sorted(par), "Element {:?} is not sorted", par);
    }
    assert_eq!(
        iterable,
        &iterable
            .iter()
            .map(|par| ParSortMatcher::sort_match(par).term)
            .collect::<Vec<_>>()[..],
        "Iterable is not sorted correctly"
    );
}

fn is_sorted(par: &Par) -> bool { *par == ParSortMatcher::sort_match(par).term }

fn check_sorted_input<A, B, F>(f: F, unsorted: A, sorted: A)
where
    A: Clone + PartialEq + std::fmt::Debug,
    B: PartialEq + std::fmt::Debug,
    F: Fn(A) -> B,
{
    assert_eq!(
        f(sorted.clone()),
        f(unsorted.clone()),
        "Function did not return the same result for sorted and unsorted inputs"
    );
}
