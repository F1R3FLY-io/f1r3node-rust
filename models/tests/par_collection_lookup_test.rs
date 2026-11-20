// The direct lookups must answer exactly what a lookup in the sorted
// collections built from the same protobuf values answers, byte for byte.

use std::collections::{HashMap, HashSet};

use models::rhoapi::{EMap, ESet, KeyValuePair, Par};
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::{
    new_elist_par, new_gint_expr, new_gint_par, new_gstring_expr, new_gstring_par,
};
use prost::Message;

fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn text(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

fn int_with_locally_free(value: i64, locally_free: Vec<u8>) -> Par {
    new_gint_par(value, locally_free, false)
}

fn par_of(exprs: &[i64]) -> Par {
    Par::default().with_exprs(exprs.iter().map(|v| new_gint_expr(*v)).collect())
}

fn mixed_par() -> Par {
    Par::default().with_exprs(vec![
        new_gstring_expr("b".to_string()),
        new_gint_expr(7),
        new_gstring_expr("a".to_string()),
    ])
}

fn list(ps: Vec<Par>) -> Par { new_elist_par(ps, Vec::new(), false, None, Vec::new(), false) }

fn emap(entries: Vec<(Par, Par)>) -> EMap {
    EMap {
        kvs: entries
            .into_iter()
            .map(|(k, v)| KeyValuePair {
                key: Some(k),
                value: Some(v),
            })
            .collect(),
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }
}

fn eset(ps: Vec<Par>) -> ESet {
    ESet {
        ps,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }
}

fn maps() -> Vec<EMap> {
    vec![
        emap(Vec::new()),
        emap(vec![(int(1), text("one")), (int(2), text("two"))]),
        emap(vec![
            (par_of(&[1, 2]), text("p")),
            (par_of(&[2, 1]), text("q")),
        ]),
        emap(vec![
            (par_of(&[2, 1]), text("q")),
            (par_of(&[1, 2]), text("p")),
        ]),
        emap(vec![(int(5), text("first")), (int(5), text("second"))]),
        emap(vec![
            (int_with_locally_free(9, vec![1]), text("lf1")),
            (int_with_locally_free(9, vec![2]), text("lf2")),
        ]),
        emap(vec![
            (mixed_par(), par_of(&[3, 1, 2])),
            (
                list(vec![int(1), par_of(&[4, 3])]),
                list(vec![par_of(&[6, 5])]),
            ),
            (text("k"), mixed_par()),
        ]),
        emap(
            (0..40)
                .map(|i| (par_of(&[i % 7, i]), list(vec![int(i), par_of(&[i, i + 1])])))
                .collect(),
        ),
    ]
}

fn queries(map: &EMap) -> Vec<Par> {
    let mut queries: Vec<Par> = map.kvs.iter().filter_map(|kv| kv.key.clone()).collect();
    queries.extend([
        par_of(&[1, 2]),
        par_of(&[2, 1]),
        int(5),
        int_with_locally_free(9, vec![3]),
        int(404),
        text("missing"),
        list(vec![int(1), par_of(&[3, 4])]),
        Par::default(),
    ]);
    queries
}

fn sorted(par: &Par) -> Par { ParSortMatcher::sort_match(par).term }

#[test]
fn map_lookup_answers_what_the_sorted_map_answers() {
    for map in maps() {
        let index: HashMap<Par, Par> = ParMapTypeMapper::emap_to_par_map(map.clone())
            .ps
            .sorted_list
            .into_iter()
            .collect();
        for query in queries(&map) {
            let expected = index.get(&sorted(&query));
            let actual = ParMapTypeMapper::lookup(&map, &query);
            assert_eq!(
                actual.as_ref().map(Message::encode_to_vec),
                expected.map(Message::encode_to_vec),
                "lookup of {query:?} in {map:?}"
            );
            assert_eq!(
                ParMapTypeMapper::contains_key(&map, &query),
                expected.is_some(),
                "contains_key of {query:?} in {map:?}"
            );
        }
    }
}

#[test]
fn set_contains_answers_what_the_sorted_set_answers() {
    let sets = vec![
        eset(Vec::new()),
        eset(vec![int(1), int(2), int(1)]),
        eset(vec![par_of(&[1, 2]), par_of(&[2, 1]), mixed_par()]),
        eset(vec![
            int_with_locally_free(9, vec![1]),
            list(vec![par_of(&[4, 3])]),
        ]),
        eset((0..40).map(|i| par_of(&[i % 5, i])).collect()),
    ];
    let probes = vec![
        int(1),
        int(3),
        par_of(&[2, 1]),
        par_of(&[1, 2]),
        mixed_par(),
        int_with_locally_free(9, vec![7]),
        list(vec![par_of(&[3, 4])]),
        par_of(&[4, 39]),
        par_of(&[39, 4]),
        Par::default(),
    ];

    for set in sets {
        let index: HashSet<Par> = ParSetTypeMapper::eset_to_par_set(set.clone())
            .ps
            .sorted_pars
            .into_iter()
            .collect();
        for probe in &probes {
            assert_eq!(
                ParSetTypeMapper::contains(&set, probe),
                index.contains(&sorted(probe)),
                "contains of {probe:?} in {set:?}"
            );
        }
    }
}
