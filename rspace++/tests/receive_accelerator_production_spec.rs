use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed, TestRunner};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::ordered_receive::{PreparedContinuationRecord, RestingReceiveVisit};
use rspace_plus_plus::rspace::receive_accelerator::{ReceiveAnswer, ReceiveKind as ProductionKind};
use rspace_plus_plus::rspace::rho_path::ReceiveChannelQuery;

#[path = "support/receive_occurrence_reference.rs"]
#[allow(dead_code)]
mod reference;

#[path = "support/receive_accelerator_production.rs"]
mod production;

use production::{
    Build, ProductionAccelerator, RawChannel, RawId, RawOccurrences, RawState, RawStore, RawTarget,
    StringContinuation, ordered_targets, raw_hot_store, raw_hot_store_from, reference_kind,
    reference_target,
};
use reference::{
    AcceleratorClass, AcceleratorUnderTest, Answer, ChannelImage, GroupKey, Lineage, Observation,
    Occurrence, ReceiveKind, ReceiveTarget, ReferenceWorld, capacity, derive, group_key, observe,
    occurrences_of,
};

const IMAGES: [&[u8]; 5] = [&[], &[0x00], &[0x01], &[0x01, 0x00], &[0x01, 0x00, 0xFF]];
const ABSENT_IMAGE: &[u8] = &[0x01, 0x01];
const BUILDS: [Build; 2] = [Build::Published, Build::Detached];
const STORE_CASES: u32 = 256;
const PROPERTY_SEED: u64 = 41_318_011;
const COLLIDING_PEEK_BYTE: i32 = 0x0100_0000;
const FORGED_PEEK_BYTE: i32 = 0x0200_0000;

fn raw_channels(images: &[&[u8]]) -> Vec<RawChannel> {
    images
        .iter()
        .map(|image| RawChannel(image.to_vec()))
        .collect()
}

fn indexed_channels(indices: &[usize]) -> Vec<RawChannel> {
    indices
        .iter()
        .map(|index| match IMAGES.get(*index) {
            Some(image) => RawChannel(image.to_vec()),
            None => panic!("generated image index {index} is outside IMAGES"),
        })
        .collect()
}

fn raw_key(channels: &[RawChannel]) -> GroupKey {
    channels.iter().map(|channel| channel.0.clone()).collect()
}

fn channels_of(group: &GroupKey) -> Vec<RawChannel> {
    group
        .iter()
        .map(|image| RawChannel(image.clone()))
        .collect()
}

fn raw_continuation(
    channels: &[RawChannel],
    body: &str,
    persist: bool,
    peeks: BTreeSet<i32>,
) -> StringContinuation {
    let patterns: Vec<String> = (0..channels.len())
        .map(|position| format!("pattern-{position}"))
        .collect();
    WaitingContinuation::create(&channels.to_vec(), &patterns, &body.to_owned(), persist, peeks)
}

fn alias_of(continuation: &StringContinuation) -> StringContinuation {
    let mut alias = continuation.clone();
    alias.source.hash = Blake2b256Hash::from_bytes(vec![0x5A; 32]);
    alias
}

fn flip_last_peek_byte(peeks: &BTreeSet<i32>, mask: i32) -> BTreeSet<i32> {
    let mut flipped = peeks.clone();
    let last = flipped
        .pop_last()
        .expect("an identity's last byte is the high byte of its last peek");
    flipped.insert(last ^ mask);
    flipped
}

fn assert_differs_only_in_last_byte(left: &[u8], right: &[u8]) {
    assert_eq!(left.len(), right.len(), "the forged bytes keep their length");
    match (left.split_last(), right.split_last()) {
        (Some((left_last, left_prefix)), Some((right_last, right_prefix))) => {
            assert_eq!(left_prefix, right_prefix, "the forged bytes share every byte but the last");
            assert_ne!(left_last, right_last, "the forged bytes differ in their last byte");
        }
        _ => panic!("a continuation record field is never empty"),
    }
}

fn checked_increment(count: usize) -> usize {
    count.checked_add(1).expect("a test tally fits usize")
}

fn probes() -> Vec<ChannelImage> {
    let capacity = IMAGES
        .len()
        .checked_add(1)
        .expect("the probe count fits usize");
    let mut probes = Vec::with_capacity(capacity);
    probes.extend(IMAGES.iter().map(|image| image.to_vec()));
    probes.push(ABSENT_IMAGE.to_vec());
    probes
}

fn hydrated(store: &RawStore, universe: &[Vec<RawChannel>]) -> Observation {
    observe(&**store, universe, true)
}

fn world_of(store: &RawStore, universe: &[Vec<RawChannel>]) -> ReferenceWorld {
    Lineage::new(Vec::new()).world(hydrated(store, universe))
}

fn rebuilt<'a>(
    store: &'a RawStore,
    build: Build,
    world: &ReferenceWorld,
) -> ProductionAccelerator<'a> {
    let mut accelerator = ProductionAccelerator::new(&**store, build);
    accelerator.rebuild(world);
    accelerator
}

fn exact(answer: Answer) -> BTreeSet<Occurrence> {
    match answer {
        Answer::Exact(occurrences) => occurrences,
        Answer::Fallback(class) => {
            panic!("a hydrated, rebuilt store answers exactly, not Fallback({class:?})")
        }
    }
}

fn exact_production(accelerator: &ProductionAccelerator<'_>, image: &[u8]) -> RawOccurrences {
    match accelerator.occurrences(&RawChannel(image.to_vec())) {
        ReceiveAnswer::Exact(occurrences) => occurrences,
        ReceiveAnswer::Fallback(class) => {
            panic!("a hydrated, rebuilt store answers exactly, not Fallback({class:?})")
        }
    }
}

fn answers_at(accelerator: &ProductionAccelerator<'_>, world: &ReferenceWorld) -> Vec<Answer> {
    let images = probes();
    let mut answers = Vec::with_capacity(images.len());
    answers.extend(images.iter().map(|image| accelerator.answer(world, image)));
    answers
}

fn answered_targets(occurrences: &RawOccurrences) -> Vec<(GroupKey, RawTarget)> {
    let count = occurrences
        .groups()
        .iter()
        .map(|group| group.targets().len())
        .fold(0usize, |total, count| {
            total
                .checked_add(count)
                .expect("an answer's target count fits usize")
        });
    let mut targets = Vec::with_capacity(count);
    for group in occurrences.groups() {
        let key = group_key(group.channels());
        targets.extend(
            group
                .targets()
                .iter()
                .map(|target| (key.clone(), target.clone())),
        );
    }
    targets
}

fn resolved_targets(store: &RawStore, occurrences: &RawOccurrences) -> BTreeSet<ReceiveTarget> {
    let mut resolved = BTreeSet::new();
    for (key, target) in answered_targets(occurrences) {
        let found = store
            .resolve_receive(&occurrences.receive_id(&target))
            .expect("a current receive id resolves");
        assert!(found == target, "a receive id resolves to its own full bytes");
        resolved.insert(reference_target(&key, &found));
    }
    resolved
}

fn assert_ids_resolve_only_to_their_own_targets(store: &RawStore, occurrences: &RawOccurrences) {
    let answered = answered_targets(occurrences);
    for (index, (_, target)) in answered.iter().enumerate() {
        let found = store
            .resolve_receive(&occurrences.receive_id(target))
            .expect("a current receive id resolves");
        for (other_index, (_, other)) in answered.iter().enumerate() {
            assert_eq!(
                found == *other,
                index == other_index,
                "a receive id resolves only to its own target"
            );
        }
    }
}

fn current_ids(accelerator: &ProductionAccelerator<'_>) -> Vec<(RawId, ReceiveTarget)> {
    let mut ids = Vec::new();
    for image in probes() {
        let occurrences = exact_production(accelerator, &image);
        let answered = answered_targets(&occurrences);
        ids.reserve(answered.len());
        ids.extend(
            answered.iter().map(|(key, target)| {
                (occurrences.receive_id(target), reference_target(key, target))
            }),
        );
    }
    ids
}

fn all_answered_targets(
    accelerator: &ProductionAccelerator<'_>,
    world: &ReferenceWorld,
) -> BTreeSet<ReceiveTarget> {
    let mut targets = BTreeSet::new();
    for image in probes() {
        targets.extend(
            exact(accelerator.answer(world, &image))
                .into_iter()
                .map(|occurrence| occurrence.target),
        );
    }
    targets
}

fn foreign_target(
    kind: ReceiveKind,
    channels: &[RawChannel],
    continuation: &StringContinuation,
) -> RawTarget {
    let store = raw_hot_store();
    match kind {
        ReceiveKind::Resting => {
            assert_eq!(store.put_continuation(channels, continuation.clone()), Some(true))
        }
        ReceiveKind::Installed => {
            assert_eq!(store.install_continuation(channels, continuation.clone()), Some(()))
        }
    }
    let world = world_of(&store, &[channels.to_vec()]);
    let accelerator = rebuilt(&store, Build::Detached, &world);
    let image = channels
        .first()
        .expect("a receive group has a first channel");
    let answered = answered_targets(&exact_production(&accelerator, &image.0));
    match answered.as_slice() {
        [(_, target)] => target.clone(),
        _ => panic!("a store holding one receive answers exactly one target"),
    }
}

fn forged_targets(
    kind: ReceiveKind,
    channels: &[RawChannel],
    continuation: &StringContinuation,
) -> Vec<RawTarget> {
    let original = expected_target(kind, channels, continuation);
    let mut identity_flipped = continuation.clone();
    identity_flipped.peeks = flip_last_peek_byte(&continuation.peeks, FORGED_PEEK_BYTE);
    assert_differs_only_in_last_byte(
        &original.identity,
        &expected_target(kind, channels, &identity_flipped).identity,
    );
    let mut source_flipped = continuation.clone();
    source_flipped.source.persistent = !source_flipped.source.persistent;
    let source_target = expected_target(kind, channels, &source_flipped);
    assert_eq!(original.identity, source_target.identity, "a source forgery keeps the identity");
    assert_differs_only_in_last_byte(&original.source, &source_target.source);
    let mut extended = channels.to_vec();
    extended.push(RawChannel(ABSENT_IMAGE.to_vec()));
    let regrouped = raw_continuation(
        &extended,
        &continuation.continuation,
        continuation.persist,
        continuation.peeks.clone(),
    );
    let swapped = match kind {
        ReceiveKind::Installed => ReceiveKind::Resting,
        ReceiveKind::Resting => ReceiveKind::Installed,
    };
    vec![
        foreign_target(kind, channels, &identity_flipped),
        foreign_target(kind, channels, &source_flipped),
        foreign_target(kind, &extended, &regrouped),
        foreign_target(swapped, channels, continuation),
    ]
}

fn resting_view_orders(store: &RawStore, channels: &[RawChannel]) -> BTreeSet<u64> {
    let mut orders = BTreeSet::new();
    let visit = store.try_visit_resting_receive_views(channels, &mut |record_orders, admission| {
        assert!(admission.is_ok(), "a stored continuation record admits its legacy view");
        orders.extend(record_orders.iter().copied());
    });
    assert!(matches!(visit, RestingReceiveVisit::Visited(_)), "a stored group is loaded");
    orders
}

fn expected_positions(group: &[Vec<u8>], image: &[u8]) -> Vec<u64> {
    let mut positions = Vec::with_capacity(group.len());
    let mut position = 0u64;
    for channel in group {
        if channel.as_slice() == image {
            positions.push(position);
        }
        position = position
            .checked_add(1)
            .expect("a receive position fits u64");
    }
    positions
}

fn expected_target(
    kind: ReceiveKind,
    channels: &[RawChannel],
    continuation: &StringContinuation,
) -> ReceiveTarget {
    let prepared = PreparedContinuationRecord::encode_waiting(continuation);
    let record = prepared.record_view();
    ReceiveTarget {
        kind,
        group: raw_key(channels),
        identity: record.identity().to_vec(),
        source: record.source_suffix().to_vec(),
    }
}

fn assert_record_view_positions(
    store: &RawStore,
    group: &GroupKey,
    image: &[u8],
    positions: &[u64],
) -> usize {
    let mut visited = 0usize;
    let visit = store.try_visit_resting_receive_views(&channels_of(group), &mut |_, admission| {
        let view = admission.expect("a stored continuation record admits its legacy view");
        let mut observed = Vec::with_capacity(positions.len());
        observed.extend(
            view.positions(ReceiveChannelQuery::LegacyV1(image))
                .expect("a legacy continuation view answers a legacy channel query")
                .map(|position| position.get()),
        );
        assert_eq!(
            observed, positions,
            "every record's view positions equal the occurrence positions"
        );
        visited = checked_increment(visited);
    });
    match visit {
        RestingReceiveVisit::Visited(count) => {
            assert_eq!(count, visited, "the resting visit reports every record it visited")
        }
        RestingReceiveVisit::Unloaded => panic!("an answered group is loaded"),
    }
    visited
}

fn property_runner(cases: u32) -> TestRunner {
    TestRunner::new(Config {
        cases,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        ..Config::default()
    })
}

fn check_property<S: Strategy>(
    cases: u32,
    strategy: &S,
    property: impl Fn(S::Value) -> Result<(), TestCaseError>,
) {
    let mut runner = property_runner(cases);
    match runner.run(strategy, property) {
        Ok(()) => {}
        Err(failure) => panic!("the property failed: {failure}"),
    }
}

#[derive(Clone, Debug)]
struct RecordPlan {
    body: u8,
    persist: bool,
    peek_first: bool,
}

#[derive(Clone, Debug)]
struct GroupPlan {
    images: Vec<usize>,
    records: Vec<RecordPlan>,
    installed: Option<RecordPlan>,
}

fn record_plan() -> impl Strategy<Value = RecordPlan> {
    (any::<u8>(), any::<bool>(), any::<bool>()).prop_map(|(body, persist, peek_first)| RecordPlan {
        body,
        persist,
        peek_first,
    })
}

fn group_plan() -> impl Strategy<Value = GroupPlan> {
    (
        proptest::collection::vec(0..IMAGES.len(), 1..=4),
        proptest::collection::vec(record_plan(), 1..=3),
        proptest::option::of(record_plan()),
    )
        .prop_map(|(images, records, installed)| GroupPlan {
            images,
            records,
            installed,
        })
}

fn store_plan() -> impl Strategy<Value = Vec<GroupPlan>> {
    proptest::collection::vec(group_plan(), 1..=4)
}

fn planned_continuation(channels: &[RawChannel], plan: &RecordPlan) -> StringContinuation {
    let peeks = match plan.peek_first {
        true => BTreeSet::from([0]),
        false => BTreeSet::new(),
    };
    raw_continuation(channels, &format!("body-{}", plan.body), plan.persist, peeks)
}

fn build_store_in_order(plans: &[GroupPlan], reverse_puts: bool) -> RawStore {
    let store = raw_hot_store();
    let put_count = plans
        .iter()
        .map(|plan| plan.records.len())
        .fold(0usize, |total, count| {
            total
                .checked_add(count)
                .expect("the planned put count fits usize")
        });
    let mut puts: Vec<(Vec<RawChannel>, StringContinuation)> = Vec::with_capacity(put_count);
    for plan in plans {
        let channels = indexed_channels(&plan.images);
        for record in &plan.records {
            puts.push((channels.clone(), planned_continuation(&channels, record)));
        }
    }
    if reverse_puts {
        puts.reverse();
    }
    for (channels, continuation) in puts {
        let outcome = store.put_continuation(&channels, continuation);
        assert!(outcome.is_some(), "an in-memory put reports whether it inserted");
    }
    for plan in plans {
        if let Some(record) = &plan.installed {
            let channels = indexed_channels(&plan.images);
            let installed = planned_continuation(&channels, record);
            assert_eq!(store.install_continuation(&channels, installed), Some(()));
        }
    }
    store
}

fn build_planned_store(plans: &[GroupPlan]) -> RawStore { build_store_in_order(plans, false) }

fn planned_universe(plans: &[GroupPlan]) -> Vec<Vec<RawChannel>> {
    let mut groups: BTreeMap<Vec<usize>, Vec<RawChannel>> = BTreeMap::new();
    for plan in plans {
        groups
            .entry(plan.images.clone())
            .or_insert_with(|| indexed_channels(&plan.images));
    }
    groups.into_values().collect()
}

fn planned_probes(plans: &[GroupPlan]) -> BTreeSet<ChannelImage> {
    let mut images: BTreeSet<ChannelImage> = probes().into_iter().collect();
    for plan in plans {
        images.extend(raw_key(&indexed_channels(&plan.images)));
    }
    images
}

fn expected_targets(plans: &[GroupPlan]) -> BTreeMap<GroupKey, BTreeSet<ReceiveTarget>> {
    let mut targets: BTreeMap<GroupKey, BTreeSet<ReceiveTarget>> = BTreeMap::new();
    let mut installed: BTreeMap<GroupKey, ReceiveTarget> = BTreeMap::new();
    for plan in plans {
        let channels = indexed_channels(&plan.images);
        let key = raw_key(&channels);
        for record in &plan.records {
            let continuation = planned_continuation(&channels, record);
            targets
                .entry(key.clone())
                .or_default()
                .insert(expected_target(ReceiveKind::Resting, &channels, &continuation));
        }
        if let Some(record) = &plan.installed {
            let continuation = planned_continuation(&channels, record);
            installed.insert(
                key.clone(),
                expected_target(ReceiveKind::Installed, &channels, &continuation),
            );
        }
    }
    for (key, target) in installed {
        targets.entry(key).or_default().insert(target);
    }
    targets
}

fn expected_occurrences(plans: &[GroupPlan], image: &[u8]) -> BTreeSet<Occurrence> {
    let mut occurrences = BTreeSet::new();
    for (group, targets) in expected_targets(plans) {
        let positions = expected_positions(&group, image);
        match positions.is_empty() {
            true => {}
            false => {
                for target in targets {
                    occurrences.insert(Occurrence {
                        channel: image.to_vec(),
                        group: group.clone(),
                        positions: positions.clone(),
                        target,
                    });
                }
            }
        }
    }
    occurrences
}

fn assert_ordered_targets_follow_production(
    store: &RawStore,
    world: &ReferenceWorld,
    accelerator: &ProductionAccelerator<'_>,
    channels: &[RawChannel],
) -> Result<(), TestCaseError> {
    let key = raw_key(channels);
    let installed = world.observation.installed.contains_key(&key);
    let continuations = store.get_continuations(channels);
    let mut production = Vec::with_capacity(continuations.len());
    production.extend(
        continuations
            .iter()
            .enumerate()
            .map(|(index, continuation)| {
                let kind = match (installed, index) {
                    (true, 0) => ReceiveKind::Installed,
                    _ => ReceiveKind::Resting,
                };
                expected_target(kind, channels, continuation)
            }),
    );
    let image = channels
        .first()
        .expect("a receive group has a first channel");
    let answered = ordered_targets(&exact_production(accelerator, &image.0))
        .into_iter()
        .find_map(|(group, targets)| (group == key).then_some(targets))
        .expect("a stored group answers at its first channel");
    prop_assert_eq!(
        answered,
        production,
        "the ordered answer targets equal the production historical order"
    );
    Ok(())
}

fn assert_construction_order_invariance(plans: &[GroupPlan]) -> Result<(), TestCaseError> {
    let universe = planned_universe(plans);
    let forward = build_store_in_order(plans, false);
    let reverse = build_store_in_order(plans, true);
    let forward_world = world_of(&forward, &universe);
    let reverse_world = world_of(&reverse, &universe);
    let forward_accelerator = rebuilt(&forward, Build::Published, &forward_world);
    let reverse_accelerator = rebuilt(&reverse, Build::Published, &reverse_world);
    for image in planned_probes(plans) {
        prop_assert_eq!(
            forward_accelerator.answer(&forward_world, &image),
            reverse_accelerator.answer(&reverse_world, &image),
            "the answers are independent of construction order"
        );
        prop_assert_eq!(
            resolved_targets(&forward, &exact_production(&forward_accelerator, &image)),
            resolved_targets(&reverse, &exact_production(&reverse_accelerator, &image)),
            "the ids of both orders resolve to the same targets"
        );
    }
    for channels in &universe {
        assert_ordered_targets_follow_production(
            &forward,
            &forward_world,
            &forward_accelerator,
            channels,
        )?;
        assert_ordered_targets_follow_production(
            &reverse,
            &reverse_world,
            &reverse_accelerator,
            channels,
        )?;
    }
    Ok(())
}

#[test]
fn production_answers_equal_the_derivation_over_generated_stores() {
    check_property(STORE_CASES, &store_plan(), |plans| {
        let store = build_planned_store(&plans);
        let world = world_of(&store, &planned_universe(&plans));
        let published = rebuilt(&store, Build::Published, &world);
        let detached = rebuilt(&store, Build::Detached, &world);
        for image in planned_probes(&plans) {
            let expected = Answer::Exact(expected_occurrences(&plans, &image));
            prop_assert_eq!(&Answer::Exact(derive(&world.observation, &image)), &expected);
            prop_assert_eq!(&published.answer(&world, &image), &expected);
            prop_assert_eq!(&detached.answer(&world, &image), &expected);
        }
        Ok(())
    });
}

#[test]
fn production_positions_come_from_the_group_prefix() {
    check_property(STORE_CASES, &store_plan(), |plans| {
        let store = build_planned_store(&plans);
        let world = world_of(&store, &planned_universe(&plans));
        let accelerator = rebuilt(&store, Build::Published, &world);
        for image in planned_probes(&plans) {
            let mut groups: BTreeMap<GroupKey, (Vec<u64>, usize)> = BTreeMap::new();
            for occurrence in exact(accelerator.answer(&world, &image)) {
                prop_assert_eq!(
                    &occurrence.positions,
                    &expected_positions(&occurrence.group, &image)
                );
                prop_assert_eq!(capacity(&occurrence.group, &image), occurrence.positions.len());
                prop_assert!(
                    occurrences_of(&occurrence.target).contains(&occurrence),
                    "an occurrence is its target's own projection at the probed channel"
                );
                let resting = matches!(occurrence.target.kind, ReceiveKind::Resting);
                let entry = groups
                    .entry(occurrence.group)
                    .or_insert_with(|| (occurrence.positions.clone(), 0));
                prop_assert_eq!(&entry.0, &occurrence.positions);
                entry.1 = entry
                    .1
                    .checked_add(usize::from(resting))
                    .expect("a record tally fits usize");
            }
            for (group, (positions, resting)) in &groups {
                prop_assert_eq!(
                    assert_record_view_positions(&store, group, &image, positions),
                    *resting
                );
            }
        }
        Ok(())
    });
}

#[test]
fn production_repeated_channels_yield_one_occurrence_per_target() {
    let store = raw_hot_store();
    let a: &[u8] = &[0x01];
    let b: &[u8] = &[0x00];
    let aba = raw_channels(&[a, b, a]);
    let aab = raw_channels(&[a, a, b]);
    let aba_record = raw_continuation(&aba, "repeated-aba", false, BTreeSet::new());
    let aab_record = raw_continuation(&aab, "repeated-aab", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&aba, aba_record.clone()), Some(true));
    assert_eq!(store.put_continuation(&aab, aab_record.clone()), Some(true));
    let world = world_of(&store, &[aba.clone(), aab.clone()]);
    let aba_target = expected_target(ReceiveKind::Resting, &aba, &aba_record);
    let aab_target = expected_target(ReceiveKind::Resting, &aab, &aab_record);
    let expected_a = BTreeSet::from([
        Occurrence {
            channel: a.to_vec(),
            group: aba_target.group.clone(),
            positions: vec![0, 2],
            target: aba_target.clone(),
        },
        Occurrence {
            channel: a.to_vec(),
            group: aab_target.group.clone(),
            positions: vec![0, 1],
            target: aab_target.clone(),
        },
    ]);
    let expected_b = BTreeSet::from([
        Occurrence {
            channel: b.to_vec(),
            group: aba_target.group.clone(),
            positions: vec![1],
            target: aba_target.clone(),
        },
        Occurrence {
            channel: b.to_vec(),
            group: aab_target.group.clone(),
            positions: vec![2],
            target: aab_target.clone(),
        },
    ]);
    for build in BUILDS {
        let accelerator = rebuilt(&store, build, &world);
        assert_eq!(accelerator.answer(&world, a), Answer::Exact(expected_a.clone()));
        assert_eq!(accelerator.answer(&world, b), Answer::Exact(expected_b.clone()));
        assert_eq!(accelerator.answer(&world, ABSENT_IMAGE), Answer::Exact(BTreeSet::new()));
    }
    assert_eq!(capacity(&aba_target.group, a), 2);
    assert_eq!(capacity(&aab_target.group, a), 2);
    assert_eq!(capacity(&aab_target.group, b), 1);
}

#[test]
fn production_prefix_aliased_images_never_share_occurrences() {
    let store = raw_hot_store();
    let groups: [Vec<RawChannel>; 4] = [
        raw_channels(&[&[0x01]]),
        raw_channels(&[&[0x01, 0x00]]),
        raw_channels(&[&[0x01, 0x00, 0xFF], &[]]),
        raw_channels(&[&[0x00]]),
    ];
    for (index, channels) in groups.iter().enumerate() {
        let record = raw_continuation(channels, &format!("prefix-{index}"), false, BTreeSet::new());
        assert_eq!(store.put_continuation(channels, record), Some(true));
    }
    let world = world_of(&store, &groups);
    for build in BUILDS {
        let accelerator = rebuilt(&store, build, &world);
        for image in IMAGES {
            let occurrences = exact(accelerator.answer(&world, image));
            let expected_groups: BTreeSet<GroupKey> = groups
                .iter()
                .map(|channels| raw_key(channels))
                .filter(|key| key.iter().any(|channel| channel.as_slice() == image))
                .collect();
            let answered_groups: BTreeSet<GroupKey> = occurrences
                .iter()
                .map(|occurrence| occurrence.group.clone())
                .collect();
            assert_eq!(
                answered_groups, expected_groups,
                "an image reaches exactly the groups holding it"
            );
            assert_eq!(
                occurrences,
                derive(&world.observation, image),
                "the production answer equals the derivation"
            );
            assert_eq!(occurrences.len(), 1, "each image holds exactly one stored receive");
            for occurrence in &occurrences {
                assert_eq!(occurrence.channel.as_slice(), image);
                assert_eq!(occurrence.positions, expected_positions(&occurrence.group, image));
            }
        }
        assert_eq!(accelerator.answer(&world, ABSENT_IMAGE), Answer::Exact(BTreeSet::new()));
    }
}

#[test]
fn production_answers_ignore_data_and_authority_orders() {
    let store = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![channels.clone()];
    for body in ["order-first", "order-second", "order-third"] {
        let record = raw_continuation(&channels, body, false, BTreeSet::new());
        assert_eq!(store.put_continuation(&channels, record), Some(true));
    }
    assert_eq!(store.remove_continuation(&channels, 1), Some(()));
    let removed_orders = resting_view_orders(&store, &channels);
    assert_eq!(removed_orders, BTreeSet::from([1, 3]));
    let removed = world_of(&store, &universe);
    let accelerator = rebuilt(&store, Build::Published, &removed);
    let answers = answers_at(&accelerator, &removed);
    for (image, answer) in probes().iter().zip(&answers) {
        assert_eq!(answer, &Answer::Exact(derive(&removed.observation, image)));
    }
    let revision = store.receive_revision();
    let data_channel = channels
        .first()
        .cloned()
        .expect("the group has a first channel");
    store.put_datum(&data_channel, Datum::create(&data_channel, "resting-datum".to_owned(), false));
    assert!(store.receive_revision() == revision, "data never moves the receive revision");
    let with_data = world_of(&store, &universe);
    assert_eq!(
        answers_at(&accelerator, &with_data),
        answers,
        "a held handle answers unchanged after a datum"
    );
    store.set_state(store.snapshot());
    let renumbered_orders = resting_view_orders(&store, &channels);
    assert_eq!(renumbered_orders, BTreeSet::from([1, 2]));
    assert_ne!(renumbered_orders, removed_orders, "set_state renumbers the authority orders");
    let renumbered = world_of(&store, &universe);
    for answer in answers_at(&accelerator, &renumbered) {
        assert_eq!(
            answer,
            Answer::Fallback(AcceleratorClass::Foreign),
            "set_state retires the held handle"
        );
    }
    for build in BUILDS {
        assert_eq!(
            answers_at(&rebuilt(&store, build, &renumbered), &renumbered),
            answers,
            "renumbering keeps the answers"
        );
    }
}

#[test]
fn production_construction_order_does_not_change_answers_or_ids() {
    let fixed = vec![
        GroupPlan {
            images: vec![2, 1, 2],
            records: vec![
                RecordPlan {
                    body: 1,
                    persist: false,
                    peek_first: false,
                },
                RecordPlan {
                    body: 2,
                    persist: false,
                    peek_first: false,
                },
                RecordPlan {
                    body: 3,
                    persist: true,
                    peek_first: true,
                },
            ],
            installed: Some(RecordPlan {
                body: 9,
                persist: true,
                peek_first: false,
            }),
        },
        GroupPlan {
            images: vec![3],
            records: vec![
                RecordPlan {
                    body: 4,
                    persist: false,
                    peek_first: false,
                },
                RecordPlan {
                    body: 5,
                    persist: false,
                    peek_first: true,
                },
            ],
            installed: None,
        },
    ];
    assert_construction_order_invariance(&fixed).expect("the fixed plan is order invariant");
    check_property(STORE_CASES, &store_plan(), |plans| {
        assert_construction_order_invariance(&plans)
    });
}

#[test]
fn production_receive_ids_resolve_only_to_byte_identical_records() {
    let store = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let first = raw_continuation(&channels, "resolve-first", false, BTreeSet::from([0]));
    let second = raw_continuation(&channels, "resolve-second", true, BTreeSet::from([1]));
    let bucket = raw_continuation(&channels, "resolve-bucket", false, BTreeSet::from([2]));
    let mut colliding = bucket.clone();
    colliding.peeks = flip_last_peek_byte(&bucket.peeks, COLLIDING_PEEK_BYTE);
    let installed = raw_continuation(&channels, "resolve-installed", true, BTreeSet::from([0]));
    for record in [&first, &second, &bucket, &colliding] {
        assert_eq!(store.put_continuation(&channels, record.clone()), Some(true));
    }
    assert_eq!(store.install_continuation(&channels, installed.clone()), Some(()));
    let bucket_target = expected_target(ReceiveKind::Resting, &channels, &bucket);
    let colliding_target = expected_target(ReceiveKind::Resting, &channels, &colliding);
    assert_differs_only_in_last_byte(&bucket_target.identity, &colliding_target.identity);
    let expected = BTreeSet::from([
        expected_target(ReceiveKind::Resting, &channels, &first),
        expected_target(ReceiveKind::Resting, &channels, &second),
        bucket_target,
        colliding_target,
        expected_target(ReceiveKind::Installed, &channels, &installed),
    ]);
    let world = world_of(&store, &[channels.clone()]);
    for build in BUILDS {
        let accelerator = rebuilt(&store, build, &world);
        for image in [&[0x01][..], &[0x00][..]] {
            let occurrences = exact_production(&accelerator, image);
            assert_eq!(resolved_targets(&store, &occurrences), expected);
            assert_ids_resolve_only_to_their_own_targets(&store, &occurrences);
            let answered = answered_targets(&occurrences);
            for (_, target) in &answered {
                let kind = reference_kind(target.kind());
                for forged in forged_targets(kind, &channels, target.continuation()) {
                    assert!(
                        answered.iter().all(|(_, current)| *current != forged),
                        "a forgery differs from every current target"
                    );
                    assert!(
                        store
                            .resolve_receive(&occurrences.receive_id(&forged))
                            .is_none(),
                        "a forged id never resolves"
                    );
                }
            }
        }
    }
}

#[test]
fn production_metadata_distinct_records_keep_distinct_ids() {
    let store = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let base = raw_continuation(&channels, "metadata", false, BTreeSet::new());
    let persistent = raw_continuation(&channels, "metadata", true, BTreeSet::new());
    let peeking = raw_continuation(&channels, "metadata", false, BTreeSet::from([1]));
    let rebodied = raw_continuation(&channels, "metadata-rebodied", false, BTreeSet::new());
    for record in [&base, &persistent, &peeking, &rebodied] {
        assert_eq!(store.put_continuation(&channels, record.clone()), Some(true));
    }
    assert_eq!(store.install_continuation(&channels, base.clone()), Some(()));
    let expected = BTreeSet::from([
        expected_target(ReceiveKind::Resting, &channels, &base),
        expected_target(ReceiveKind::Resting, &channels, &persistent),
        expected_target(ReceiveKind::Resting, &channels, &peeking),
        expected_target(ReceiveKind::Resting, &channels, &rebodied),
        expected_target(ReceiveKind::Installed, &channels, &base),
    ]);
    let world = world_of(&store, &[channels.clone()]);
    let accelerator = rebuilt(&store, Build::Published, &world);
    let alias_target = foreign_target(ReceiveKind::Resting, &channels, &alias_of(&base));
    for image in [&[0x01][..], &[0x00][..]] {
        let occurrences = exact_production(&accelerator, image);
        let answered = answered_targets(&occurrences);
        assert_eq!(
            answered.len(),
            5,
            "persist, peeks, the continuation and the kind each keep a target"
        );
        let resting_identities: BTreeSet<Vec<u8>> = answered
            .iter()
            .filter(|(_, target)| target.kind() == ProductionKind::Resting)
            .map(|(_, target)| target.identity_bytes().into_owned())
            .collect();
        assert_eq!(
            resting_identities.len(),
            4,
            "persist, peeks and the continuation each keep a distinct identity"
        );
        assert_eq!(resolved_targets(&store, &occurrences), expected);
        assert_ids_resolve_only_to_their_own_targets(&store, &occurrences);
        let representative = answered
            .iter()
            .map(|(_, target)| target)
            .find(|target| {
                target.kind() == ProductionKind::Resting &&
                    target.identity_bytes() == alias_target.identity_bytes()
            })
            .expect("the first source of the aliased identity answers");
        assert_ne!(representative.source_bytes(), alias_target.source_bytes());
        assert!(*representative != alias_target, "a source alias keeps a distinct target");
        assert!(
            store
                .resolve_receive(&occurrences.receive_id(&alias_target))
                .is_none(),
            "an alias id never resolves to the first source"
        );
    }
}

#[test]
fn production_ids_and_handles_from_a_prior_revision_or_generation_never_answer() {
    let seeded = raw_channels(&[&[0x01]]);
    let mutated = raw_channels(&[&[0x01, 0x00]]);
    let mut state = RawState::default();
    state
        .continuations
        .insert(seeded.clone(), vec![raw_continuation(
            &seeded,
            "prior-seeded",
            false,
            BTreeSet::new(),
        )]);
    let store = raw_hot_store_from(state);
    let universe = vec![seeded.clone(), mutated.clone()];
    let initial_world = world_of(&store, &universe);
    let initial = rebuilt(&store, Build::Published, &initial_world);
    let initial_ids = current_ids(&initial);
    assert!(!initial_ids.is_empty(), "the seeded group answers");
    for (id, _) in &initial_ids {
        assert!(
            store
                .resolve_receive(id)
                .is_some_and(|found| found == *id.target()),
            "a current id resolves to its own target"
        );
    }
    let mutation = raw_continuation(&mutated, "prior-mutation", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&mutated, mutation), Some(true));
    let mutated_world = world_of(&store, &universe);
    for answer in answers_at(&initial, &mutated_world) {
        assert_eq!(
            answer,
            Answer::Fallback(AcceleratorClass::Stale),
            "a handle from before a store mutation never answers"
        );
    }
    let after_mutation = rebuilt(&store, Build::Published, &mutated_world);
    let mutated_targets = all_answered_targets(&after_mutation, &mutated_world);
    for (id, target) in &initial_ids {
        assert!(mutated_targets.contains(target), "the seeded record is still stored");
        assert!(store.resolve_receive(id).is_none(), "a prior-revision id never resolves");
    }
    let mutated_ids = current_ids(&after_mutation);
    assert!(mutated_ids.len() > initial_ids.len(), "the mutation adds an answered target");
    for (id, _) in &mutated_ids {
        assert!(
            store
                .resolve_receive(id)
                .is_some_and(|found| found == *id.target()),
            "a current id resolves to its own target"
        );
    }
    store.set_state(store.snapshot());
    let replaced_world = world_of(&store, &universe);
    for accelerator in [&initial, &after_mutation] {
        for answer in answers_at(accelerator, &replaced_world) {
            assert_eq!(
                answer,
                Answer::Fallback(AcceleratorClass::Foreign),
                "a handle from before set_state never answers"
            );
        }
    }
    let replaced = rebuilt(&store, Build::Published, &replaced_world);
    assert_eq!(
        all_answered_targets(&replaced, &replaced_world),
        mutated_targets,
        "set_state keeps the answered targets"
    );
    for (id, target) in mutated_ids.iter().chain(initial_ids.iter()) {
        assert!(mutated_targets.contains(target), "the record is still stored");
        assert!(store.resolve_receive(id).is_none(), "a prior-generation id never resolves");
    }
}

#[test]
fn production_alias_puts_keep_the_revision_answers_and_first_source() {
    let store = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![channels.clone()];
    let first = raw_continuation(&channels, "representative", false, BTreeSet::new());
    let alias = alias_of(&first);
    assert_eq!(store.put_continuation(&channels, first.clone()), Some(true));
    let before = world_of(&store, &universe);
    let accelerator = rebuilt(&store, Build::Published, &before);
    let answers = answers_at(&accelerator, &before);
    let revision = store.receive_revision();
    assert_eq!(store.put_continuation(&channels, alias.clone()), Some(false));
    assert!(store.receive_revision() == revision, "an alias put keeps the receive revision");
    let after = world_of(&store, &universe);
    assert_eq!(
        answers_at(&accelerator, &after),
        answers,
        "a held handle answers unchanged after an alias put"
    );
    let first_target = expected_target(ReceiveKind::Resting, &channels, &first);
    let alias_target = expected_target(ReceiveKind::Resting, &channels, &alias);
    assert_ne!(first_target, alias_target);
    for image in [&[0x01][..], &[0x00][..]] {
        let targets: BTreeSet<ReceiveTarget> = exact(accelerator.answer(&after, image))
            .into_iter()
            .map(|occurrence| occurrence.target)
            .collect();
        assert_eq!(
            targets,
            BTreeSet::from([first_target.clone()]),
            "the first source stays the representative"
        );
    }
    for build in BUILDS {
        assert_eq!(
            answers_at(&rebuilt(&store, build, &after), &after),
            answers,
            "a rebuild after an alias put answers unchanged"
        );
    }
}
