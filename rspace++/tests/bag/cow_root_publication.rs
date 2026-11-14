use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::iter;

use proptest::collection::vec;
use proptest::prelude::*;

use super::{Bag, BagDelta, Multiplicity};

#[derive(Clone, Copy, Debug)]
enum Construction {
    PrefixLowerThenExactHigher,
    PrefixHigherThenExactLower,
    PrefixBulkLowerThenHigher,
    PrefixBulkHigherThenLower,
    ExactLowerThenHigher,
    ExactHigherThenLower,
}

#[derive(Clone, Copy, Debug)]
enum Ownership {
    Unique,
    SharedDropSourceFirst,
    SharedDropSuccessorFirst,
}

fn long_same_node_keys() -> ([Vec<u8>; 2], Vec<u8>) {
    let mut lower = vec![0xa5; 939];
    let mut higher = lower.clone();
    lower[203] = 0x11;
    higher[203] = 0x22;
    let prefix = lower[..203].to_vec();
    assert_eq!(
        lower
            .iter()
            .zip(&higher)
            .take_while(|(left, right)| left == right)
            .count(),
        prefix.len()
    );
    ([lower, higher], prefix)
}

fn insert_count(bag: &mut Bag, path: &[u8], count: u64) {
    bag.insert_many(path, Multiplicity::new(count).unwrap())
        .unwrap();
}

fn construct(
    keys: &[Vec<u8>; 2],
    prefix: &[u8],
    counts: [u64; 2],
    construction: Construction,
) -> Bag {
    let mut result = Bag::new();
    match construction {
        Construction::PrefixLowerThenExactHigher => {
            let mut prepared = Bag::new();
            insert_count(&mut prepared, &keys[0][prefix.len()..], counts[0]);
            result.replace_prefix_with_relative(prefix, &prepared, false);
            insert_count(&mut result, &keys[1], counts[1]);
        }
        Construction::PrefixHigherThenExactLower => {
            let mut prepared = Bag::new();
            insert_count(&mut prepared, &keys[1][prefix.len()..], counts[1]);
            result.replace_prefix_with_relative(prefix, &prepared, false);
            insert_count(&mut result, &keys[0], counts[0]);
        }
        Construction::PrefixBulkLowerThenHigher | Construction::PrefixBulkHigherThenLower => {
            let mut prepared = Bag::new();
            let order = match construction {
                Construction::PrefixBulkLowerThenHigher => [0, 1],
                Construction::PrefixBulkHigherThenLower => [1, 0],
                _ => unreachable!(),
            };
            for index in order {
                insert_count(&mut prepared, &keys[index][prefix.len()..], counts[index]);
            }
            result.replace_prefix_with_relative(prefix, &prepared, false);
        }
        Construction::ExactLowerThenHigher | Construction::ExactHigherThenLower => {
            let order = match construction {
                Construction::ExactLowerThenHigher => [0, 1],
                Construction::ExactHigherThenLower => [1, 0],
                _ => unreachable!(),
            };
            for index in order {
                insert_count(&mut result, &keys[index], counts[index]);
            }
        }
    }
    result
}

fn expected_bag(entries: &BTreeMap<Vec<u8>, u64>) -> Bag {
    let mut result = Bag::new();
    for (path, count) in entries {
        insert_count(&mut result, path, *count);
    }
    result
}

fn observed_entries(bag: &Bag) -> Vec<(Vec<u8>, u64)> {
    bag.iter()
        .map(|(path, multiplicity)| (path, multiplicity.get()))
        .collect()
}

fn assert_all_observations(
    actual: &Bag,
    expected: &BTreeMap<Vec<u8>, u64>,
    explicit_absent: &[&[u8]],
    prefixes: &[Vec<u8>],
) {
    for (path, count) in expected {
        assert_eq!(actual.count(path), *count, "point lookup for {path:?}");
    }
    for path in explicit_absent {
        assert_eq!(actual.count(path), 0, "absent point lookup for {path:?}");
    }

    let expected_entries = expected
        .iter()
        .map(|(path, count)| (path.clone(), *count))
        .collect::<Vec<_>>();
    assert_eq!(observed_entries(actual), expected_entries, "full iterator");
    assert_eq!(actual.distinct_len(), expected.len(), "distinct metadata");
    assert_eq!(
        actual.exact_cardinality(),
        expected.values().map(|count| u128::from(*count)).sum(),
        "cardinality metadata"
    );
    assert_eq!(actual.prefix_value_count(b""), expected.len(), "root zipper value count");

    for prefix in prefixes {
        let expected_prefix = expected_entries
            .iter()
            .filter(|(path, _)| path.starts_with(prefix))
            .cloned()
            .collect::<Vec<_>>();
        let actual_prefix = actual
            .entries_with_prefix(prefix)
            .into_iter()
            .map(|(path, multiplicity)| (path, multiplicity.get()))
            .collect::<Vec<_>>();
        assert_eq!(actual_prefix, expected_prefix, "prefix traversal at {prefix:?}");
        assert_eq!(
            actual.prefix_value_count(prefix),
            expected_prefix.len(),
            "prefix zipper value count at {prefix:?}"
        );
    }

    let rebuilt = expected_bag(expected);
    assert_eq!(actual.compact_bytes(), rebuilt.compact_bytes(), "canonical ACT bytes");
    let decoded = Bag::from_compact_bytes(&actual.compact_bytes()).unwrap();
    assert_eq!(observed_entries(&decoded), expected_entries, "ACT decode endpoints");
    assert_eq!(decoded.compact_bytes(), actual.compact_bytes(), "ACT byte round trip");
}

#[test]
fn destructive_publication_is_coherent_for_every_mixed_construction_and_cow_order() {
    let (keys, prefix) = long_same_node_keys();
    let constructions = [
        Construction::PrefixLowerThenExactHigher,
        Construction::PrefixHigherThenExactLower,
        Construction::PrefixBulkLowerThenHigher,
        Construction::PrefixBulkHigherThenLower,
        Construction::ExactLowerThenHigher,
        Construction::ExactHigherThenLower,
    ];
    let ownerships =
        [Ownership::Unique, Ownership::SharedDropSourceFirst, Ownership::SharedDropSuccessorFirst];
    let prefixes =
        vec![Vec::new(), prefix.clone(), keys[0][..204].to_vec(), keys[1][..204].to_vec(), vec![
            0xff,
        ]];

    for construction in constructions {
        for selected in [0usize, 1] {
            for (counts, removal) in [([1, 1], 1), ([3, 5], 1), ([3, 5], 3)] {
                for ownership in ownerships {
                    let mut actual = construct(&keys, &prefix, counts, construction);
                    let before = BTreeMap::from([
                        (keys[0].clone(), counts[0]),
                        (keys[1].clone(), counts[1]),
                    ]);
                    assert_all_observations(&actual, &before, &[], &prefixes);
                    let source_identity = actual.anchor_identity();
                    let source = (!matches!(ownership, Ownership::Unique)).then(|| actual.clone());
                    let mut expected = before.clone();
                    let selected_count = expected[&keys[selected]];
                    let removes_endpoint = selected_count == removal;
                    if removes_endpoint {
                        expected.remove(&keys[selected]);
                    } else {
                        expected.insert(keys[selected].clone(), selected_count - removal);
                    }
                    let absent = removes_endpoint
                        .then_some(keys[selected].as_slice())
                        .into_iter()
                        .collect::<Vec<_>>();

                    let mut delta = BagDelta::new();
                    delta
                        .record_removal(&keys[selected], Multiplicity::new(removal).unwrap())
                        .unwrap();
                    delta.apply_to(&mut actual).unwrap();
                    if matches!(ownership, Ownership::Unique) && !removes_endpoint {
                        assert_eq!(
                            actual.anchor_identity(),
                            source_identity,
                            "a unique value-only decrement retains its write-ready anchor: \
                             {construction:?}, selected={selected}"
                        );
                    } else {
                        assert_ne!(
                            actual.anchor_identity(),
                            source_identity,
                            "shared value updates and every structural deletion publish a private \
                             candidate: {construction:?}, selected={selected}, \
                             ownership={ownership:?}"
                        );
                    }
                    assert_all_observations(&actual, &expected, &absent, &prefixes);

                    if let Some(source) = source {
                        assert_eq!(source.anchor_identity(), source_identity);
                        assert_all_observations(&source, &before, &[], &prefixes);
                        match ownership {
                            Ownership::SharedDropSourceFirst => {
                                drop(source);
                                assert_all_observations(&actual, &expected, &absent, &prefixes);
                            }
                            Ownership::SharedDropSuccessorFirst => {
                                drop(actual);
                                assert_all_observations(&source, &before, &[], &prefixes);
                            }
                            Ownership::Unique => unreachable!(),
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn unique_value_only_updates_retain_the_anchor_across_bag_apis() {
    fn fixture() -> Bag {
        let mut bag = Bag::new();
        insert_count(&mut bag, b"shared/prefix/selected", 7);
        insert_count(&mut bag, b"shared/prefix/sibling", 11);
        bag
    }

    let selected = b"shared/prefix/selected";
    let decrement = Multiplicity::new(2).unwrap();

    let mut via_remove_many = fixture();
    let anchor = via_remove_many.anchor_identity();
    assert_eq!(
        via_remove_many
            .remove_many(selected, decrement)
            .unwrap()
            .get(),
        5
    );
    assert_eq!(via_remove_many.anchor_identity(), anchor);

    let mut via_try_remove_many = fixture();
    let anchor = via_try_remove_many.anchor_identity();
    assert_eq!(
        via_try_remove_many
            .try_remove_many(selected, decrement)
            .unwrap()
            .unwrap()
            .get(),
        5
    );
    assert_eq!(via_try_remove_many.anchor_identity(), anchor);

    let mut via_set_count = fixture();
    let anchor = via_set_count.anchor_identity();
    via_set_count.set_count(selected, Some(Multiplicity::new(5).unwrap()));
    assert_eq!(via_set_count.anchor_identity(), anchor);

    let mut via_delta = fixture();
    let anchor = via_delta.anchor_identity();
    let mut delta = BagDelta::new();
    delta.record_removal(selected, decrement).unwrap();
    delta.apply_to(&mut via_delta).unwrap();
    assert_eq!(via_delta.anchor_identity(), anchor);
}

#[test]
fn shared_value_only_updates_detach_without_mutating_the_alias() {
    let mut source = Bag::new();
    insert_count(&mut source, b"shared/prefix/selected", 7);
    insert_count(&mut source, b"shared/prefix/sibling", 11);
    let source_anchor = source.anchor_identity();
    let source_bytes = source.compact_bytes();

    let mut successor = source.clone();
    assert_eq!(successor.anchor_identity(), source_anchor);
    assert_eq!(
        successor
            .remove_many(b"shared/prefix/selected", Multiplicity::new(2).unwrap())
            .unwrap()
            .get(),
        5
    );

    assert_ne!(successor.anchor_identity(), source_anchor);
    assert_eq!(source.anchor_identity(), source_anchor);
    assert_eq!(source.compact_bytes(), source_bytes);
    assert_eq!(source.count(b"shared/prefix/selected"), 7);
    assert_eq!(source.count(b"shared/prefix/sibling"), 11);
    assert_eq!(successor.count(b"shared/prefix/selected"), 5);
    assert_eq!(successor.count(b"shared/prefix/sibling"), 11);
}

const ALIAS_ONLY_KEY: &[u8] = b"\xfealias-only";

type Observation = fn(&Bag, &BTreeMap<Vec<u8>, u64>, &[&[u8]], &[Vec<u8>]);

#[derive(Clone, Copy, Debug)]
enum ExhaustingApi {
    TryRemoveMany,
    RemoveMany,
    Remove,
    SetCountNone,
    DeltaCancellation,
}

const EXHAUSTING_APIS: [ExhaustingApi; 5] = [
    ExhaustingApi::TryRemoveMany,
    ExhaustingApi::RemoveMany,
    ExhaustingApi::Remove,
    ExhaustingApi::SetCountNone,
    ExhaustingApi::DeltaCancellation,
];

const BAG_EXHAUSTING_APIS: [ExhaustingApi; 4] = [
    ExhaustingApi::TryRemoveMany,
    ExhaustingApi::RemoveMany,
    ExhaustingApi::Remove,
    ExhaustingApi::SetCountNone,
];

impl ExhaustingApi {
    fn exhausted_count(self) -> u64 {
        match self {
            Self::Remove => 1,
            Self::TryRemoveMany |
            Self::RemoveMany |
            Self::SetCountNone |
            Self::DeltaCancellation => 5,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RemovalOwnership {
    Unique,
    SharedDropSourceFirst,
    SharedDropSuccessorFirst,
    CloneDroppedBeforeRemoval,
    CloneMutatedThenDropped,
}

const REMOVAL_OWNERSHIPS: [RemovalOwnership; 5] = [
    RemovalOwnership::Unique,
    RemovalOwnership::SharedDropSourceFirst,
    RemovalOwnership::SharedDropSuccessorFirst,
    RemovalOwnership::CloneDroppedBeforeRemoval,
    RemovalOwnership::CloneMutatedThenDropped,
];

#[derive(Clone, Copy, Debug)]
enum RemovalPath {
    InPlace,
    Rebuilt,
}

impl RemovalOwnership {
    fn removal_path(self) -> RemovalPath {
        match self {
            Self::Unique | Self::CloneDroppedBeforeRemoval => RemovalPath::InPlace,
            Self::SharedDropSourceFirst |
            Self::SharedDropSuccessorFirst |
            Self::CloneMutatedThenDropped => RemovalPath::Rebuilt,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum InsertionOrder {
    SelectedBeforeRetained,
    RetainedBeforeSelected,
}

fn multiplicity(count: u64) -> Multiplicity {
    Multiplicity::new(count).expect("a positive test count is a multiplicity")
}

fn assert_removal_path(actual: &Bag, source_identity: usize, path: RemovalPath, context: &str) {
    match path {
        RemovalPath::InPlace => assert_eq!(
            actual.anchor_identity(),
            source_identity,
            "an exclusive exhausting removal retains its anchor: {context}"
        ),
        RemovalPath::Rebuilt => assert_ne!(
            actual.anchor_identity(),
            source_identity,
            "a shared or header-forked anchor falls back to a rebuilt anchor: {context}"
        ),
    }
}

#[derive(Clone)]
enum RemovalSubject {
    Bag(Bag),
    Delta(BagDelta),
}

impl RemovalSubject {
    fn build(api: ExhaustingApi, entries: &[(Vec<u8>, u64)]) -> Self {
        let mut subject = match api {
            ExhaustingApi::DeltaCancellation => Self::Delta(BagDelta::new()),
            ExhaustingApi::TryRemoveMany |
            ExhaustingApi::RemoveMany |
            ExhaustingApi::Remove |
            ExhaustingApi::SetCountNone => Self::Bag(Bag::new()),
        };
        for (path, count) in entries {
            subject.write(path, *count);
        }
        subject
    }

    fn observed(&self) -> &Bag {
        match self {
            Self::Bag(bag) => bag,
            Self::Delta(delta) => delta.additions(),
        }
    }

    fn write(&mut self, path: &[u8], count: u64) {
        match self {
            Self::Bag(bag) => insert_count(bag, path, count),
            Self::Delta(delta) => delta
                .record_addition(path, multiplicity(count))
                .expect("a bounded test addition fits u64"),
        }
    }

    fn exhaust(&mut self, api: ExhaustingApi, path: &[u8], count: u64) {
        match (self, api) {
            (Self::Bag(bag), ExhaustingApi::TryRemoveMany) => assert_eq!(
                bag.try_remove_many(path, multiplicity(count))
                    .expect("the whole resident count is available"),
                None,
                "an exact exhausting try_remove_many reports no remainder"
            ),
            (Self::Bag(bag), ExhaustingApi::RemoveMany) => assert_eq!(
                bag.remove_many(path, multiplicity(count)),
                None,
                "an exhausting remove_many reports no remainder"
            ),
            (Self::Bag(bag), ExhaustingApi::Remove) => {
                assert_eq!(count, 1, "remove exhausts only a singleton endpoint");
                assert_eq!(bag.remove(path), None, "an exhausting remove reports no remainder");
            }
            (Self::Bag(bag), ExhaustingApi::SetCountNone) => bag.set_count(path, None),
            (Self::Delta(delta), ExhaustingApi::DeltaCancellation) => {
                delta
                    .record_removal(path, multiplicity(count))
                    .expect("a removal within the recorded addition cancels it");
                assert!(
                    delta.removals().is_empty(),
                    "an exact cancellation records no removal remainder"
                );
            }
            (_, api) => unreachable!("RemovalSubject::build gives {api:?} its own subject kind"),
        }
    }
}

struct ExhaustionCase<'a> {
    before: &'a BTreeMap<Vec<u8>, u64>,
    selected: &'a [u8],
    api: ExhaustingApi,
    ownership: RemovalOwnership,
    prefixes: &'a [Vec<u8>],
    context: &'a str,
    observe: Observation,
}

fn prepare_ownership(
    subject: &RemovalSubject,
    case: &ExhaustionCase<'_>,
) -> Option<RemovalSubject> {
    let ExhaustionCase {
        before,
        ownership,
        prefixes,
        context,
        observe,
        ..
    } = *case;
    let identity = subject.observed().anchor_identity();
    match ownership {
        RemovalOwnership::Unique => None,
        RemovalOwnership::SharedDropSourceFirst | RemovalOwnership::SharedDropSuccessorFirst => {
            Some(subject.clone())
        }
        RemovalOwnership::CloneDroppedBeforeRemoval => {
            let transient = subject.clone();
            assert_eq!(
                transient.observed().anchor_identity(),
                identity,
                "a wrapper clone shares its source's anchor: {context}"
            );
            None
        }
        RemovalOwnership::CloneMutatedThenDropped => {
            let mut fork = subject.clone();
            fork.write(ALIAS_ONLY_KEY, 1);
            assert_eq!(
                subject.observed().anchor_identity(),
                identity,
                "a write through a clone leaves its source's anchor in place: {context}"
            );
            assert_ne!(
                fork.observed().anchor_identity(),
                identity,
                "a write through a shared anchor forks a private header: {context}"
            );
            let mut forked = before.clone();
            forked.insert(ALIAS_ONLY_KEY.to_vec(), 1);
            observe(fork.observed(), &forked, &[], prefixes);
            drop(fork);
            observe(subject.observed(), before, &[ALIAS_ONLY_KEY], prefixes);
            None
        }
    }
}

fn assert_alias_untouched(
    alias: &RemovalSubject,
    source_identity: usize,
    case: &ExhaustionCase<'_>,
) {
    let ExhaustionCase {
        before,
        prefixes,
        context,
        observe,
        ..
    } = *case;
    assert_eq!(
        alias.observed().anchor_identity(),
        source_identity,
        "an exhausting removal never moves its alias's anchor: {context}"
    );
    observe(alias.observed(), before, &[], prefixes);
}

fn assert_exclusive_survivor(
    mut survivor: RemovalSubject,
    mut entries: BTreeMap<Vec<u8>, u64>,
    count: u64,
    case: &ExhaustionCase<'_>,
) {
    let ExhaustionCase {
        selected,
        api,
        prefixes,
        context,
        observe,
        ..
    } = *case;
    let identity = survivor.observed().anchor_identity();
    match entries.insert(selected.to_vec(), count) {
        Some(resident) => assert_eq!(
            resident, count,
            "the surviving alias still holds the selected count: {context}"
        ),
        None => {
            survivor.write(selected, count);
            assert_eq!(
                survivor.observed().anchor_identity(),
                identity,
                "a write to the surviving anchor never forks it: {context}"
            );
        }
    }
    observe(survivor.observed(), &entries, &[], prefixes);
    entries.remove(selected);
    survivor.exhaust(api, selected, count);
    assert_eq!(
        survivor.observed().anchor_identity(),
        identity,
        "the surviving anchor admits an in-place exhausting removal: {context}"
    );
    observe(survivor.observed(), &entries, &[selected], prefixes);
}

fn exhaust_and_check(mut subject: RemovalSubject, case: &ExhaustionCase<'_>) {
    let ExhaustionCase {
        before,
        selected,
        api,
        ownership,
        prefixes,
        context,
        observe,
    } = *case;
    observe(subject.observed(), before, &[], prefixes);
    let source_identity = subject.observed().anchor_identity();
    let alias = prepare_ownership(&subject, case);
    let mut expected = before.clone();
    let count = expected
        .remove(selected)
        .expect("the selected endpoint is resident before its removal");
    subject.exhaust(api, selected, count);
    assert_removal_path(subject.observed(), source_identity, ownership.removal_path(), context);
    observe(subject.observed(), &expected, &[selected], prefixes);
    let (survivor, survivor_entries) = match (alias, ownership) {
        (Some(alias), RemovalOwnership::SharedDropSourceFirst) => {
            assert_alias_untouched(&alias, source_identity, case);
            drop(alias);
            observe(subject.observed(), &expected, &[selected], prefixes);
            (subject, expected)
        }
        (Some(alias), RemovalOwnership::SharedDropSuccessorFirst) => {
            assert_alias_untouched(&alias, source_identity, case);
            drop(subject);
            observe(alias.observed(), before, &[], prefixes);
            (alias, before.clone())
        }
        (
            None,
            RemovalOwnership::Unique |
            RemovalOwnership::CloneDroppedBeforeRemoval |
            RemovalOwnership::CloneMutatedThenDropped,
        ) => (subject, expected),
        (_, ownership) => unreachable!(
            "prepare_ownership keeps an alias for exactly the shared cases, not {ownership:?}"
        ),
    };
    assert_exclusive_survivor(survivor, survivor_entries, count, case);
}

struct ExhaustionShape {
    name: &'static str,
    retained: Vec<(Vec<u8>, u64)>,
    selected: Vec<u8>,
    prefixes: Vec<Vec<u8>>,
}

fn exhaustion_shapes() -> [ExhaustionShape; 6] {
    let ([lower, higher], fork_prefix) = long_same_node_keys();
    let long_prefixes =
        vec![Vec::new(), fork_prefix, lower[..204].to_vec(), higher[..204].to_vec()];
    [
        ExhaustionShape {
            name: "fork sibling leaf",
            retained: vec![(b"shared/prefix/sibling".to_vec(), 11)],
            selected: b"shared/prefix/selected".to_vec(),
            prefixes: vec![
                Vec::new(),
                b"shared/prefix/".to_vec(),
                b"shared/prefix/s".to_vec(),
                vec![0xff],
            ],
        },
        ExhaustionShape {
            name: "valued ancestor leaf",
            retained: vec![(b"shared/prefix".to_vec(), 13)],
            selected: b"shared/prefix/selected".to_vec(),
            prefixes: vec![Vec::new(), b"shared/prefix".to_vec(), b"shared/prefix/".to_vec()],
        },
        ExhaustionShape {
            name: "interior endpoint",
            retained: vec![
                (b"shared/prefix/child".to_vec(), 13),
                (b"shared/prefix/other".to_vec(), 17),
            ],
            selected: b"shared/prefix".to_vec(),
            prefixes: vec![Vec::new(), b"shared/prefix".to_vec(), b"shared/prefix/c".to_vec()],
        },
        ExhaustionShape {
            name: "root endpoint",
            retained: vec![(b"a".to_vec(), 3), (b"b/c".to_vec(), 7)],
            selected: Vec::new(),
            prefixes: vec![Vec::new(), b"a".to_vec(), b"b".to_vec()],
        },
        ExhaustionShape {
            name: "lone key",
            retained: Vec::new(),
            selected: b"shared/only".to_vec(),
            prefixes: vec![Vec::new(), b"shared/".to_vec()],
        },
        ExhaustionShape {
            name: "long same-node leaf",
            retained: vec![(higher, 11)],
            selected: lower,
            prefixes: long_prefixes,
        },
    ]
}

#[test]
fn exhausting_removal_keeps_every_alias_coherent_across_removal_apis() {
    for ExhaustionShape {
        name,
        retained,
        selected,
        prefixes,
    } in exhaustion_shapes()
    {
        for order in
            [InsertionOrder::SelectedBeforeRetained, InsertionOrder::RetainedBeforeSelected]
        {
            for api in EXHAUSTING_APIS {
                let selected_entry = (selected.clone(), api.exhausted_count());
                let entries = match order {
                    InsertionOrder::SelectedBeforeRetained => iter::once(selected_entry)
                        .chain(retained.iter().cloned())
                        .collect::<Vec<_>>(),
                    InsertionOrder::RetainedBeforeSelected => retained
                        .iter()
                        .cloned()
                        .chain(iter::once(selected_entry))
                        .collect::<Vec<_>>(),
                };
                let before = entries.iter().cloned().collect::<BTreeMap<_, _>>();
                for ownership in REMOVAL_OWNERSHIPS {
                    let context = format!("{name}, {order:?}, {api:?}, {ownership:?}");
                    exhaust_and_check(RemovalSubject::build(api, &entries), &ExhaustionCase {
                        before: &before,
                        selected: &selected,
                        api,
                        ownership,
                        prefixes: &prefixes,
                        context: &context,
                        observe: assert_all_observations,
                    });
                }
            }
        }
    }
}

#[test]
fn exhausting_removal_is_coherent_for_every_mixed_construction() {
    let (keys, prefix) = long_same_node_keys();
    let constructions = [
        Construction::PrefixLowerThenExactHigher,
        Construction::PrefixHigherThenExactLower,
        Construction::PrefixBulkLowerThenHigher,
        Construction::PrefixBulkHigherThenLower,
        Construction::ExactLowerThenHigher,
        Construction::ExactHigherThenLower,
    ];
    let prefixes =
        [Vec::new(), prefix.clone(), keys[0][..204].to_vec(), keys[1][..204].to_vec(), vec![0xff]];
    for construction in constructions {
        for selected in [0usize, 1] {
            for api in BAG_EXHAUSTING_APIS {
                let mut counts = [11, 11];
                counts[selected] = api.exhausted_count();
                let before =
                    BTreeMap::from([(keys[0].clone(), counts[0]), (keys[1].clone(), counts[1])]);
                for ownership in REMOVAL_OWNERSHIPS {
                    let context =
                        format!("{construction:?}, selected={selected}, {api:?}, {ownership:?}");
                    exhaust_and_check(
                        RemovalSubject::Bag(construct(&keys, &prefix, counts, construction)),
                        &ExhaustionCase {
                            before: &before,
                            selected: &keys[selected],
                            api,
                            ownership,
                            prefixes: &prefixes,
                            context: &context,
                            observe: assert_all_observations,
                        },
                    );
                }
            }
        }
    }
}

#[test]
fn a_fallback_rebuild_restores_in_place_exhausting_removal() {
    let first: &[u8] = b"restore/first";
    let second: &[u8] = b"restore/second";
    let prefixes = [Vec::new(), b"restore/".to_vec(), b"restore/s".to_vec()];
    for api in EXHAUSTING_APIS {
        let count = api.exhausted_count();
        let entries =
            [(first.to_vec(), count), (second.to_vec(), count), (b"restore/sibling".to_vec(), 11)];
        let before = entries.iter().cloned().collect::<BTreeMap<_, _>>();
        for ownership in
            [RemovalOwnership::SharedDropSourceFirst, RemovalOwnership::CloneMutatedThenDropped]
        {
            let context = format!("{api:?}, {ownership:?}");
            let case = ExhaustionCase {
                before: &before,
                selected: first,
                api,
                ownership,
                prefixes: &prefixes,
                context: &context,
                observe: assert_all_observations,
            };
            let mut subject = RemovalSubject::build(api, &entries);
            let source_identity = subject.observed().anchor_identity();
            let alias = prepare_ownership(&subject, &case);
            let mut expected = before.clone();
            expected.remove(first);
            subject.exhaust(api, first, count);
            assert_removal_path(
                subject.observed(),
                source_identity,
                RemovalPath::Rebuilt,
                &context,
            );
            assert_all_observations(subject.observed(), &expected, &[first], &prefixes);
            let rebuilt_identity = subject.observed().anchor_identity();
            expected.remove(second);
            subject.exhaust(api, second, count);
            assert_removal_path(
                subject.observed(),
                rebuilt_identity,
                RemovalPath::InPlace,
                &context,
            );
            assert_all_observations(subject.observed(), &expected, &[first, second], &prefixes);
            if let Some(alias) = &alias {
                assert_alias_untouched(alias, source_identity, &case);
            }
        }
    }
}

fn on_small_stack(operation: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .name("bag-exhausting-removal-256k".to_owned())
        .stack_size(256 * 1024)
        .spawn(operation)
        .expect("the fixed-stack test thread must spawn");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

fn assert_deep_observations(
    actual: &Bag,
    expected: &BTreeMap<Vec<u8>, u64>,
    explicit_absent: &[&[u8]],
    prefixes: &[Vec<u8>],
) {
    for (path, count) in expected {
        let length = path.len();
        assert!(actual.count(path) == *count, "deep point lookup of a {length}-byte key");
    }
    for path in explicit_absent {
        let length = path.len();
        assert!(actual.count(path) == 0, "deep absent point lookup of a {length}-byte key");
    }
    let expected_entries = expected
        .iter()
        .map(|(path, count)| (path.clone(), *count))
        .collect::<Vec<_>>();
    assert!(observed_entries(actual) == expected_entries, "deep full iterator");
    assert_eq!(actual.distinct_len(), expected.len(), "deep distinct metadata");
    assert_eq!(
        actual.exact_cardinality(),
        expected
            .values()
            .map(|count| u128::from(*count))
            .sum::<u128>(),
        "deep cardinality metadata"
    );
    for prefix in prefixes {
        let depth = prefix.len();
        let expected_prefix = expected_entries
            .iter()
            .filter(|(path, _)| path.starts_with(prefix))
            .cloned()
            .collect::<Vec<_>>();
        let actual_prefix = actual
            .entries_with_prefix(prefix)
            .into_iter()
            .map(|(path, multiplicity)| (path, multiplicity.get()))
            .collect::<Vec<_>>();
        assert!(actual_prefix == expected_prefix, "deep prefix traversal at depth {depth}");
    }
    let image = actual.compact_bytes();
    assert!(image == expected_bag(expected).compact_bytes(), "deep canonical ACT bytes");
    let decoded = Bag::from_compact_bytes(&image).expect("a deep canonical ACT image decodes");
    assert!(observed_entries(&decoded) == expected_entries, "deep ACT decode endpoints");
    assert!(decoded.compact_bytes() == image, "deep ACT byte round trip");
}

#[derive(Clone, Copy, Debug)]
enum DeepShape {
    BesideRootFork,
    LoneChain,
}

impl DeepShape {
    fn entries(self, deep: &[u8], count: u64) -> Vec<(Vec<u8>, u64)> {
        match self {
            Self::BesideRootFork => vec![(deep.to_vec(), count), (vec![0x5b], 11)],
            Self::LoneChain => vec![(deep.to_vec(), count)],
        }
    }
}

#[test]
fn exhausting_removal_of_a_deep_key_is_stack_safe() {
    on_small_stack(|| {
        let deep: Vec<u8> = vec![0x5a; 65_536];
        let prefixes = [Vec::new(), deep[..1].to_vec(), deep[..32_768].to_vec()];
        let cases = EXHAUSTING_APIS
            .map(|api| (DeepShape::BesideRootFork, api, RemovalOwnership::Unique))
            .into_iter()
            .chain([
                (
                    DeepShape::BesideRootFork,
                    ExhaustingApi::TryRemoveMany,
                    RemovalOwnership::SharedDropSuccessorFirst,
                ),
                (DeepShape::LoneChain, ExhaustingApi::TryRemoveMany, RemovalOwnership::Unique),
            ]);
        for (shape, api, ownership) in cases {
            let entries = shape.entries(&deep, api.exhausted_count());
            let before = entries.iter().cloned().collect::<BTreeMap<_, _>>();
            let context = format!("{shape:?}, {api:?}, {ownership:?}");
            exhaust_and_check(RemovalSubject::build(api, &entries), &ExhaustionCase {
                before: &before,
                selected: &deep,
                api,
                ownership,
                prefixes: &prefixes,
                context: &context,
                observe: assert_deep_observations,
            });
        }
    });
}

const DIFFERENTIAL_ALPHABET: &[u8] = &[0x00, 0x01, 0xff];
const DIFFERENTIAL_CHAIN: &[u8] = &[0x5a, 0x5b];

#[derive(Clone, Debug)]
enum DifferentialStep {
    Insert { key: usize, amount: u64 },
    TryRemoveMany { key: usize, amount: Option<u64> },
    RemoveMany { key: usize, amount: Option<u64> },
    Remove { key: usize },
    SetCount { key: usize, count: Option<u64> },
    HoldAlias,
    ReleaseAlias { index: usize },
    ForkAlias,
}

#[derive(Clone, Copy, Debug)]
enum AnchorEffect {
    Untouched,
    Written,
    Exhausted,
    Forked,
}

enum AnchorOutcome {
    Retained,
    Replaced,
}

struct AnchorModel {
    exclusive: bool,
    epoch: u64,
}

impl AnchorModel {
    fn advance(&mut self, effect: AnchorEffect, shared: bool) -> AnchorOutcome {
        match (effect, shared, self.exclusive) {
            (AnchorEffect::Untouched, _, _) |
            (AnchorEffect::Written, false, _) |
            (AnchorEffect::Exhausted, false, true) => AnchorOutcome::Retained,
            (AnchorEffect::Written, true, _) => {
                self.exclusive = false;
                self.epoch += 1;
                AnchorOutcome::Replaced
            }
            (AnchorEffect::Exhausted, _, _) => {
                self.exclusive = true;
                self.epoch += 1;
                AnchorOutcome::Replaced
            }
            (AnchorEffect::Forked, _, _) => {
                self.exclusive = false;
                AnchorOutcome::Retained
            }
        }
    }
}

struct HeldAlias {
    bag: Bag,
    entries: BTreeMap<Vec<u8>, u64>,
    identity: usize,
    epoch: u64,
}

fn assert_held_alias(alias: &HeldAlias, prefixes: &[Vec<u8>]) {
    assert_eq!(
        alias.bag.anchor_identity(),
        alias.identity,
        "an alias keeps its anchor while the subject changes"
    );
    assert_all_observations(&alias.bag, &alias.entries, &[ALIAS_ONLY_KEY], prefixes);
}

fn differential_key() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        4 => vec(prop::sample::select(DIFFERENTIAL_ALPHABET), 0..6),
        1 => vec(prop::sample::select(DIFFERENTIAL_CHAIN), 24..48),
    ]
}

fn differential_step() -> impl Strategy<Value = DifferentialStep> {
    prop_oneof![
        4 => (any::<usize>(), 1..4u64)
            .prop_map(|(key, amount)| DifferentialStep::Insert { key, amount }),
        2 => (any::<usize>(), prop::option::of(1..5u64))
            .prop_map(|(key, amount)| DifferentialStep::TryRemoveMany { key, amount }),
        2 => (any::<usize>(), prop::option::of(1..8u64))
            .prop_map(|(key, amount)| DifferentialStep::RemoveMany { key, amount }),
        1 => any::<usize>().prop_map(|key| DifferentialStep::Remove { key }),
        2 => (any::<usize>(), prop::option::of(1..4u64))
            .prop_map(|(key, count)| DifferentialStep::SetCount { key, count }),
        1 => Just(DifferentialStep::HoldAlias),
        1 => any::<usize>().prop_map(|index| DifferentialStep::ReleaseAlias { index }),
        1 => Just(DifferentialStep::ForkAlias),
    ]
}

fn saturating_removal_effect(
    entries: &mut BTreeMap<Vec<u8>, u64>,
    path: &[u8],
    requested: u64,
    remaining: Option<Multiplicity>,
) -> AnchorEffect {
    match entries
        .get(path)
        .copied()
        .map(|resident| (requested.cmp(&resident), resident))
    {
        Some((Ordering::Less, resident)) => {
            let next = resident - requested;
            assert_eq!(
                remaining,
                Some(multiplicity(next)),
                "a partial saturating removal reports the remainder"
            );
            entries.insert(path.to_vec(), next);
            AnchorEffect::Written
        }
        Some((Ordering::Equal | Ordering::Greater, _)) => {
            assert_eq!(remaining, None, "an exhausting saturating removal reports no remainder");
            entries.remove(path);
            AnchorEffect::Exhausted
        }
        None => {
            assert_eq!(remaining, None, "a removal of an absent endpoint reports nothing");
            AnchorEffect::Untouched
        }
    }
}

struct DifferentialRun<'a> {
    pool: &'a [Vec<u8>],
    prefixes: Vec<Vec<u8>>,
    subject: Bag,
    entries: BTreeMap<Vec<u8>, u64>,
    held: Vec<HeldAlias>,
    model: AnchorModel,
}

impl<'a> DifferentialRun<'a> {
    fn new(pool: &'a [Vec<u8>], steps: usize) -> Self {
        Self {
            pool,
            prefixes: iter::once(Vec::new()).chain(pool.iter().cloned()).collect(),
            subject: Bag::new(),
            entries: BTreeMap::new(),
            held: Vec::with_capacity(steps),
            model: AnchorModel {
                exclusive: true,
                epoch: 0,
            },
        }
    }

    fn key(&self, index: usize) -> &'a [u8] {
        let pool = self.pool;
        &pool[index % pool.len()]
    }

    fn apply(&mut self, step: &DifferentialStep) -> (AnchorEffect, Option<&'a [u8]>) {
        match *step {
            DifferentialStep::Insert { key, amount } => {
                let path = self.key(key);
                let total = match self.entries.get(path).copied() {
                    Some(resident) => resident + amount,
                    None => amount,
                };
                let next = self
                    .subject
                    .insert_many(path, multiplicity(amount))
                    .expect("a bounded differential insertion fits u64");
                assert_eq!(next.get(), total, "insert_many reports the oracle's count");
                self.entries.insert(path.to_vec(), total);
                (AnchorEffect::Written, Some(path))
            }
            DifferentialStep::TryRemoveMany { key, amount } => {
                let path = self.key(key);
                let current = self.entries.get(path).copied();
                let requested = amount.or(current).unwrap_or(1);
                let outcome = self.subject.try_remove_many(path, multiplicity(requested));
                let effect = match current.map(|resident| (requested.cmp(&resident), resident)) {
                    Some((Ordering::Less, resident)) => {
                        let remaining = resident - requested;
                        assert_eq!(
                            outcome,
                            Ok(Some(multiplicity(remaining))),
                            "a partial try_remove_many reports the remainder"
                        );
                        self.entries.insert(path.to_vec(), remaining);
                        AnchorEffect::Written
                    }
                    Some((Ordering::Equal, _)) => {
                        assert_eq!(
                            outcome,
                            Ok(None),
                            "an exact try_remove_many reports no remainder"
                        );
                        self.entries.remove(path);
                        AnchorEffect::Exhausted
                    }
                    Some((Ordering::Greater, available)) => {
                        assert_eq!(
                            outcome,
                            Err(super::BagError::InsufficientMultiplicity {
                                available,
                                requested
                            }),
                            "an insufficient try_remove_many is refused without a change"
                        );
                        AnchorEffect::Untouched
                    }
                    None => {
                        assert_eq!(
                            outcome,
                            Err(super::BagError::InsufficientMultiplicity {
                                available: 0,
                                requested,
                            }),
                            "a try_remove_many of an absent endpoint is refused without a change"
                        );
                        AnchorEffect::Untouched
                    }
                };
                (effect, Some(path))
            }
            DifferentialStep::RemoveMany { key, amount } => {
                let path = self.key(key);
                let requested = amount.or(self.entries.get(path).copied()).unwrap_or(1);
                let remaining = self.subject.remove_many(path, multiplicity(requested));
                (
                    saturating_removal_effect(&mut self.entries, path, requested, remaining),
                    Some(path),
                )
            }
            DifferentialStep::Remove { key } => {
                let path = self.key(key);
                let remaining = self.subject.remove(path);
                (saturating_removal_effect(&mut self.entries, path, 1, remaining), Some(path))
            }
            DifferentialStep::SetCount { key, count } => {
                let path = self.key(key);
                let current = self.entries.get(path).copied();
                self.subject.set_count(path, count.map(multiplicity));
                let effect = match (current, count) {
                    (None, None) => AnchorEffect::Untouched,
                    (Some(_), None) => {
                        self.entries.remove(path);
                        AnchorEffect::Exhausted
                    }
                    (current, Some(next)) => match current.map(|resident| resident.cmp(&next)) {
                        Some(Ordering::Equal) => AnchorEffect::Untouched,
                        Some(Ordering::Less | Ordering::Greater) | None => {
                            self.entries.insert(path.to_vec(), next);
                            AnchorEffect::Written
                        }
                    },
                };
                (effect, Some(path))
            }
            DifferentialStep::HoldAlias => {
                self.held.push(HeldAlias {
                    bag: self.subject.clone(),
                    entries: self.entries.clone(),
                    identity: self.subject.anchor_identity(),
                    epoch: self.model.epoch,
                });
                (AnchorEffect::Untouched, None)
            }
            DifferentialStep::ReleaseAlias { index } => {
                if let Some(position) = index.checked_rem(self.held.len()) {
                    let alias = self.held.swap_remove(position);
                    assert_held_alias(&alias, &self.prefixes);
                }
                (AnchorEffect::Untouched, None)
            }
            DifferentialStep::ForkAlias => {
                let identity = self.subject.anchor_identity();
                let mut fork = self.subject.clone();
                fork.insert_many(ALIAS_ONLY_KEY, Multiplicity::ONE)
                    .expect("one alias-only key fits u64");
                assert_ne!(
                    fork.anchor_identity(),
                    identity,
                    "a write through a shared anchor forks a private header"
                );
                let mut forked = self.entries.clone();
                forked.insert(ALIAS_ONLY_KEY.to_vec(), 1);
                assert_all_observations(&fork, &forked, &[], &self.prefixes);
                (AnchorEffect::Forked, None)
            }
        }
    }
}

fn run_differential(pool: &[Vec<u8>], steps: &[DifferentialStep]) {
    let mut run = DifferentialRun::new(pool, steps.len());
    for (position, step) in steps.iter().enumerate() {
        let identity = run.subject.anchor_identity();
        let epoch = run.model.epoch;
        let shared = run.held.iter().any(|alias| alias.epoch == epoch);
        let (effect, touched) = run.apply(step);
        let context = format!("step {position} {step:?}: {effect:?}, shared={shared}");
        match run.model.advance(effect, shared) {
            AnchorOutcome::Retained => assert_eq!(
                run.subject.anchor_identity(),
                identity,
                "the anchor model predicts a retained anchor: {context}"
            ),
            AnchorOutcome::Replaced => assert_ne!(
                run.subject.anchor_identity(),
                identity,
                "the anchor model predicts a replaced anchor: {context}"
            ),
        }
        let absent = touched
            .filter(|path| !run.entries.contains_key(*path))
            .into_iter()
            .collect::<Vec<_>>();
        assert_all_observations(&run.subject, &run.entries, &absent, &run.prefixes);
    }
    for alias in run.held.drain(..) {
        assert_held_alias(&alias, &run.prefixes);
    }
    assert_all_observations(&run.subject, &run.entries, &[ALIAS_ONLY_KEY], &run.prefixes);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn exhausting_removals_match_an_independent_btreemap_oracle(
        pool in vec(differential_key(), 1..8),
        steps in vec(differential_step(), 1..48),
    ) {
        run_differential(&pool, &steps);
    }
}
