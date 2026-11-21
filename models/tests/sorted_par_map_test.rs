use std::collections::HashMap;

use models::rhoapi::{
    connective, g_unforgeable, Bundle, Connective, EMap, GPrivate, GUnforgeable, KeyValuePair, Par,
    Send,
};
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::rholang::sorter::ordering::Ordering;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::sorted_par_map::SortedParMap;
use models::rust::test_utils::test_utils::sort;
use models::rust::utils::{
    new_boundvar_par, new_eset_par, new_gint_par, new_gstring_par, new_par_from_par_set,
};
use prost::Message;

fn to_kv_pair(pair: (Par, Par)) -> KeyValuePair {
    KeyValuePair {
        key: Some(pair.0),
        value: Some(pair.1),
    }
}

fn emap_of(kvs: &[(Par, Par)]) -> EMap {
    EMap {
        kvs: kvs.iter().map(|pair| to_kv_pair(pair.clone())).collect(),
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    }
}

fn serialize_emap(map: &SortedParMap) -> Vec<u8> { emap_of(&map.sorted_list).encode_to_vec() }

fn create_pars() -> Vec<(Par, Par)> {
    vec![
        (
            new_gint_par(7, vec![], false),
            new_gstring_par("Seven".to_string(), vec![], false),
        ),
        (
            new_gint_par(7, vec![], false),
            new_gstring_par("SeVen".to_string(), vec![], false),
        ),
        (
            new_boundvar_par(1, vec![], false),
            new_boundvar_par(0, vec![], false),
        ),
        (
            new_gint_par(2, vec![], false),
            new_eset_par(
                vec![
                    new_gint_par(2, vec![], false),
                    new_gint_par(1, vec![], false),
                ],
                vec![],
                false,
                None,
                vec![],
                false,
            ),
        ),
        (
            new_gint_par(2, vec![], false),
            new_eset_par(
                vec![new_gint_par(2, vec![], false)],
                vec![],
                false,
                None,
                vec![],
                false,
            ),
        ),
    ]
}

fn round_trip_test(par_map: &SortedParMap) {
    let serialized = serialize_emap(par_map);

    let expected_emap = EMap {
        kvs: par_map
            .sorted_list
            .iter()
            .map(|pair| to_kv_pair(pair.clone()))
            .collect(),
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    };

    let deserialized = EMap::decode(&*serialized).expect("Failed to deserialize EMap");

    assert_eq!(
        deserialized, expected_emap,
        "Round trip serialization failed: deserialized EMap does not match expected"
    );
}

fn sample() -> SortedParMap {
    let pars = create_pars();
    SortedParMap::create_from_vec(pars)
}

#[test]
fn sorted_par_map_should_preserve_structure_during_round_trip_protobuf_serialization() {
    let sample_map = sample();
    round_trip_test(&sample_map);
}

#[test]
fn sorted_par_map_should_deduplicate_elements_where_last_seen_element_wins() {
    let mut expected_pairs = HashMap::new();
    expected_pairs.insert(
        new_gint_par(7, Vec::new(), false),
        new_gstring_par("SeVen".to_string(), Vec::new(), false),
    );
    expected_pairs.insert(
        new_gint_par(2, Vec::new(), false),
        new_par_from_par_set(
            vec![new_gint_par(2, Vec::new(), false)],
            Vec::new(),
            false,
            None,
        ),
    );

    let after_roundtrip_serialization = EMap::decode(&*serialize_emap(&sample()))
        .expect("Failed to deserialize EMap")
        .kvs;

    let result = expected_pairs.iter().all(|(key, value)| {
        after_roundtrip_serialization
            .iter()
            .any(|kv| kv.key.as_ref().unwrap() == key && kv.value.as_ref().unwrap() == value)
    });

    assert!(
        result,
        "Deduplication failed: expected pairs do not match serialized map"
    );
}

#[test]
fn sorted_par_map_should_preserve_ordering_during_serialization() {
    let reference_bytes = serialize_emap(&sample());

    for i in 1..=1000 {
        let serialized = serialize_emap(&sample());
        let res = serialized == reference_bytes; // Equivalent to util.Arrays.equals from Scala
        assert!(
            res,
            "Run #{} serialization failed: serialized bytes differ from reference bytes",
            i
        );
    }
}

#[test]
fn sorted_par_map_should_be_equal_when_it_is_equal() {
    let mut ps = HashMap::new();
    ps.insert(
        Par::default(),
        Par::default()
            .with_unforgeables(vec![
                GUnforgeable {
                    unf_instance: Some(g_unforgeable::UnfInstance::GPrivateBody(GPrivate {
                        id: vec![0],
                    })),
                },
                GUnforgeable {
                    unf_instance: Some(g_unforgeable::UnfInstance::GPrivateBody(
                        GPrivate::default(),
                    )),
                },
            ])
            .with_connective_used(true),
    );

    let map1 = SortedParMap::create_from_map(ps.clone());
    let map2 = SortedParMap::create_from_map(ps);

    assert_eq!(
        map1, map2,
        "SortedParMap instances created from the same elements should be equal"
    );
}

#[test]
fn sorted_par_map_should_keep_keys_sorted() {
    let sorted_par_map_keys = [
        Par::default().with_connective_used(true),
        Par::default().with_bundles(vec![Bundle {
            body: Some(Par::default()),
            write_flag: true,
            read_flag: false,
        }]),
        Par::default().with_connectives(vec![Connective {
            connective_instance: Some(connective::ConnectiveInstance::ConnBool(false)),
        }]),
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(g_unforgeable::UnfInstance::GPrivateBody(GPrivate::default())),
        }]),
        Par::default(),
    ];

    let sorted_par_map = SortedParMap::create_from_vec(
        sorted_par_map_keys
            .iter()
            .cloned()
            .zip(vec![Par::default(); sorted_par_map_keys.len()])
            .collect(),
    );

    let keys = sorted_par_map.keys();

    assert_eq!(
        keys,
        Ordering::sort_pars(&keys),
        "Keys in SortedParMap are not sorted correctly"
    );
}

#[test]
fn sorted_par_map_should_sort_all_input() {
    let keys: Vec<Par> = create_pars().iter().map(|(key, _)| key.clone()).collect();
    let values: Vec<Par> = create_pars()
        .iter()
        .map(|(_, value)| value.clone())
        .collect();
    let kvs: Vec<(Par, Par)> = keys.into_iter().zip(values).collect();

    let map = SortedParMap::create_from_vec(kvs.clone());

    if let Some((unsorted_key, value)) = kvs.first() {
        let sorted_key = sort(unsorted_key);

        check_sorted_input(
            |key| removed(&map, &key),
            unsorted_key.clone(),
            sorted_key.clone(),
        );
        check_sorted_input(
            |key| inserted(&map, &key, value),
            unsorted_key.clone(),
            sorted_key.clone(),
        );
        let emap = emap_of(&kvs);
        check_sorted_input(
            |key| ParMapTypeMapper::contains_key(&emap, &key),
            unsorted_key.clone(),
            sorted_key.clone(),
        );
        check_sorted_input(
            |key| ParMapTypeMapper::lookup(&emap, &key),
            unsorted_key.clone(),
            sorted_key.clone(),
        );
    }
}

#[test]
fn sorted_par_map_should_preserve_sortedness_for_all_operations() {
    let pars1 = create_pars();
    let pars2 = create_pars();

    let pairs1: Vec<(Par, Par)> = pars1
        .clone()
        .into_iter()
        .zip(pars2.clone())
        .map(|((k1, v1), _)| (k1, v1)) // take pairs from the first list
        .collect();

    let pairs2: Vec<(Par, Par)> = pars2
        .clone()
        .into_iter()
        .zip(pars1.clone())
        .map(|((k1, v1), _)| (k1, v1)) // take pairs from the second list
        .collect();

    let map1 = SortedParMap::create_from_vec(pairs1);
    let map2 = SortedParMap::create_from_vec(pairs2);

    check_sorted_entries(&map1.sorted_list);
    check_sorted_entries(&map2.sorted_list);

    for map in [&map1, &map2] {
        if let Some((key, value)) = map.sorted_list.first() {
            check_sorted_entries(&inserted(map, key, value));
            check_sorted_entries(&removed(map, key));
        }
    }
}

#[test]
fn canonical_map_operations_match_a_rebuilt_map() {
    let entry = |k: i64, v: &str| (new_gint_par(k, vec![], false), new_gstring_par(v.to_string(), vec![], false));
    let canonical = |entries: Vec<(Par, Par)>| SortedParMap::create_from_vec(entries).sorted_list;
    let map = SortedParMap::create_from_vec(vec![entry(1, "a"), entry(3, "c")]);

    assert_eq!(
        inserted(&map, &entry(2, "b").0, &entry(2, "b").1),
        canonical(vec![entry(1, "a"), entry(2, "b"), entry(3, "c")])
    );
    assert_eq!(
        inserted(&map, &entry(3, "z").0, &entry(3, "z").1),
        canonical(vec![entry(1, "a"), entry(3, "z")])
    );
    assert_eq!(removed(&map, &entry(1, "a").0), canonical(vec![entry(3, "c")]));
    assert_eq!(removed(&map, &entry(2, "b").0), map.sorted_list);

    let kvs = |entries: Vec<(Par, Par)>| emap_of(&canonical(entries)).kvs;
    let other = kvs(vec![entry(2, "b"), entry(3, "z")]);
    assert_eq!(
        ParMapTypeMapper::union(emap_of(&map.sorted_list).kvs, other.clone()),
        kvs(vec![entry(1, "a"), entry(2, "b"), entry(3, "z")])
    );
    assert_eq!(
        ParMapTypeMapper::diff(emap_of(&map.sorted_list).kvs, &other),
        kvs(vec![entry(1, "a")])
    );
}

fn inserted(map: &SortedParMap, key: &Par, value: &Par) -> Vec<(Par, Par)> {
    let mut kvs = emap_of(&map.sorted_list).kvs;
    ParMapTypeMapper::insert(&mut kvs, key, value);
    entries(kvs)
}

fn removed(map: &SortedParMap, key: &Par) -> Vec<(Par, Par)> {
    let mut kvs = emap_of(&map.sorted_list).kvs;
    ParMapTypeMapper::remove(&mut kvs, key);
    entries(kvs)
}

fn entries(kvs: Vec<KeyValuePair>) -> Vec<(Par, Par)> {
    kvs.into_iter()
        .map(|kv| (kv.key.unwrap(), kv.value.unwrap()))
        .collect()
}

fn check_sorted_entries(entries: &[(Par, Par)]) {
    let keys: Vec<Par> = entries.iter().map(|(key, _)| key.clone()).collect();
    check_sorted_vector_par(&keys);
    let emap = emap_of(entries);
    for (key, value) in entries {
        assert!(is_sorted(key), "Key is not sorted: {:?}", key);
        assert!(is_sorted(value), "Value is not sorted: {:?}", value);
        let fetched = ParMapTypeMapper::lookup(&emap, key).expect("every key is found");
        assert!(is_sorted(&fetched), "Fetched value is not sorted: {:?}", fetched);
    }
}

fn check_sorted_vector_par(iterable: &Vec<Par>) {
    iterable.iter().for_each(|par| {
        assert!(
            is_sorted(par),
            "Element in iterable is not sorted: {:?}",
            par
        );
    });

    let sorted_iterable = Ordering::sort_pars(iterable);

    assert_eq!(
        iterable, &sorted_iterable,
        "Iterable is not sorted: {:?}",
        iterable
    );
}

fn check_sorted_input<A, B, F>(f: F, unsorted: A, sorted: A)
where
    A: Clone + PartialEq + std::fmt::Debug,
    B: PartialEq + std::fmt::Debug,
    F: Fn(A) -> B,
{
    assert_eq!(
        f(sorted.clone()),
        f(unsorted.clone()),
        "Function output for sorted and unsorted input differs"
    );
}

fn is_sorted(par: &Par) -> bool {
    let sorted_par = ParSortMatcher::sort_match(par);
    par == &sorted_par.term
}

// Two distinct raw-form keys that canonicalize to the same Par. Each key is a
// Par carrying two Sends in opposite order; the sort matcher reorders sends
// into canonical order, so both inputs produce byte-identical canonical keys.
// Values are distinct so a swapped output order would be observable.
fn colliding_pair() -> (Par, Par, Par, Par) {
    let send1 = Send {
        chan: Some(new_gstring_par("alpha".to_string(), vec![], false)),
        data: vec![new_gint_par(1, vec![], false)],
        persistent: false,
        locally_free: vec![],
        connective_used: false,
    };
    let send2 = Send {
        chan: Some(new_gstring_par("beta".to_string(), vec![], false)),
        data: vec![new_gint_par(2, vec![], false)],
        persistent: false,
        locally_free: vec![],
        connective_used: false,
    };

    let key_a = Par::default().with_sends(vec![send1.clone(), send2.clone()]);
    let key_b = Par::default().with_sends(vec![send2, send1]);

    assert_ne!(
        key_a.encode_to_vec(),
        key_b.encode_to_vec(),
        "raw keys must differ in encoding to exercise the HashMap-ordering path"
    );
    assert_eq!(
        ParSortMatcher::sort_match(&key_a).term,
        ParSortMatcher::sort_match(&key_b).term,
        "raw keys must canonicalize to the same Par to tie on score"
    );

    let value_a = new_gstring_par("value-a".to_string(), vec![], false);
    let value_b = new_gstring_par("value-b".to_string(), vec![], false);

    (key_a, key_b, value_a, value_b)
}

#[test]
fn sorted_par_map_output_is_byte_identical_across_runs_when_raw_keys_canonicalize_the_same() {
    let (key_a, key_b, value_a, value_b) = colliding_pair();

    let build = || {
        let mut map = HashMap::new();
        map.insert(key_a.clone(), value_a.clone());
        map.insert(key_b.clone(), value_b.clone());
        serialize_emap(&SortedParMap::create_from_map(map))
    };

    // The HashMap uses a per-instance random hasher, so each `build` call
    // iterates its entries in a different order. Without a deterministic
    // tiebreaker in `Ordering::sort_map`, that iteration order leaked into
    // the serialized output. All 200 runs must now produce identical bytes.
    let baseline = build();
    for i in 0..200 {
        let next = build();
        assert_eq!(
            baseline, next,
            "run {} produced different serialized bytes — nondeterminism regression",
            i
        );
    }
}
