use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed, TestRunner};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::hot_store::{HotStoreState, ReceiveRevision};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::ordered_receive::{PreparedContinuationRecord, RestingReceiveVisit};
use rspace_plus_plus::rspace::receive_accelerator::{
    ReceiveAcceleratorClass, ReceiveAcceleratorHandle, ReceiveAnswer, ReceiveId,
    ReceiveKind as ProductionKind, ReceiveOccurrences, ReceiveTarget as ProductionTarget,
};
use rspace_plus_plus::rspace::rho_path::ReceiveChannelQuery;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::Log;
use rspace_plus_plus::rspace::trace::event::Event;
use serde::{Deserialize, Serialize};

#[path = "support/receive_occurrence_reference.rs"]
#[allow(dead_code)]
mod reference;

#[path = "support/receive_accelerator_production.rs"]
mod production;

use production::{
    Build, ProductionAccelerator, RawChannel, RawId, RawOccurrences, RawState, RawStore, RawTarget,
    StringContinuation, StubHistory, ordered_targets, raw_hot_store, raw_hot_store_from,
    raw_hot_store_with_history, reference_kind, reference_target,
};
use reference::{
    AcceleratorClass, AcceleratorUnderTest, Answer, ChannelImage, Fault, GroupKey, Lineage,
    MaintenanceOp, Observation, Occurrence, ReceiveKind, ReceiveTarget, ReferenceAccelerator,
    ReferenceWorld, StoreStep, capacity, channel_groups, channel_image, check_trace, derive,
    derive_all, group_key, observe, occurrences_of, read,
};

const IMAGES: [&[u8]; 5] = [&[], &[0x00], &[0x01], &[0x01, 0x00], &[0x01, 0x00, 0xFF]];
const ABSENT_IMAGE: &[u8] = &[0x01, 0x01];
const BUILDS: [Build; 2] = [Build::Published, Build::Detached];
const STORE_CASES: u32 = 256;
const TRACE_CASES: u32 = 64;
const PROPERTY_SEED: u64 = 41_318_011;
const COLLIDING_PEEK_BYTE: i32 = 0x0100_0000;
const FORGED_PEEK_BYTE: i32 = 0x0200_0000;
const SPACE_CHANNELS: [&str; 3] = ["space-alpha", "space-beta", "space-gamma"];

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

fn fixed_groups() -> [Vec<RawChannel>; 3] {
    [
        raw_channels(&[&[0x01]]),
        raw_channels(&[&[0x01], &[0x00], &[0x01]]),
        raw_channels(&[&[], &[0x01, 0x00]]),
    ]
}

fn answer_structure(
    answer: ReceiveAnswer<RawChannel, String, String>,
) -> Result<Vec<(GroupKey, Vec<ReceiveTarget>)>, ReceiveAcceleratorClass> {
    match answer {
        ReceiveAnswer::Exact(occurrences) => Ok(ordered_targets(&occurrences)),
        ReceiveAnswer::Fallback(class) => Err(class),
    }
}

fn answer_under_join_premise(world: &ReferenceWorld, image: &[u8]) -> Answer {
    let unloaded = world
        .observation
        .unloaded
        .iter()
        .any(|group| group.iter().any(|channel| channel.as_slice() == image));
    match unloaded {
        true => Answer::Fallback(AcceleratorClass::Incomplete),
        false => Answer::Exact(derive(&world.observation, image)),
    }
}

fn assert_maintained_equals_rebuild(
    store: &RawStore,
    maintained: &ProductionAccelerator<'_>,
    world: &ReferenceWorld,
) {
    let detached = rebuilt(store, Build::Detached, world);
    for image in probes() {
        let channel = RawChannel(image.clone());
        assert_eq!(
            answer_structure(maintained.occurrences(&channel)),
            answer_structure(detached.occurrences(&channel)),
            "the maintained accelerator answers the groups and targets of a fresh build"
        );
        assert_eq!(
            maintained.answer(world, &image),
            answer_under_join_premise(world, &image),
            "the maintained accelerator answers the derivation"
        );
    }
}

fn assert_one_revision(
    store: &RawStore,
    accelerator: &ProductionAccelerator<'_>,
    world: &ReferenceWorld,
) {
    assert_eq!(
        accelerator.artifact_revisions(),
        vec![world.revision.clone()],
        "the accelerator reports one artifact revision, the world's"
    );
    let current = current_revision(store);
    let mut resolved = 0usize;
    for image in probe_images(world) {
        match (
            accelerator.occurrences(&RawChannel(image.clone())),
            answer_under_join_premise(world, &image),
        ) {
            (ReceiveAnswer::Exact(occurrences), Answer::Exact(derived)) => {
                assert!(
                    *occurrences.revision() == current,
                    "the occurrence index answers at the store's revision"
                );
                let targets: BTreeSet<ReceiveTarget> = derived
                    .into_iter()
                    .map(|occurrence| occurrence.target)
                    .collect();
                assert_eq!(
                    resolved_targets(store, &occurrences),
                    targets,
                    "the catalog resolves every id the index mints, at the same stamp"
                );
                resolved = resolved
                    .checked_add(targets.len())
                    .expect("a resolved target count fits usize");
            }
            (
                ReceiveAnswer::Fallback(ReceiveAcceleratorClass::Incomplete),
                Answer::Fallback(AcceleratorClass::Incomplete),
            ) => {}
            (answer, expected) => panic!(
                "{image:?}: the accelerator falls back as {:?} where the world answers \
                 {expected:?}",
                fallback_class(answer)
            ),
        }
    }
    assert!(resolved > 0, "the catalog resolves some answered id");
}

fn probe_images(world: &ReferenceWorld) -> BTreeSet<ChannelImage> {
    let mut images = world.observation.channel_images();
    images.extend(probes());
    images
}

fn current_revision(store: &RawStore) -> ReceiveRevision {
    store
        .receive_revision()
        .expect("an in-memory store has a receive revision")
}

fn fallback_class<C, P: Clone, K: Clone>(
    answer: ReceiveAnswer<C, P, K>,
) -> Option<ReceiveAcceleratorClass> {
    match answer {
        ReceiveAnswer::Exact(_) => None,
        ReceiveAnswer::Fallback(class) => Some(class),
    }
}

fn assert_every_version_replaced(
    before: &Observation,
    after: &Observation,
    universe: &[Vec<RawChannel>],
) {
    for channels in universe {
        let key = group_key(channels);
        let held = before
            .version(&key)
            .expect("the held observation loaded every universe group");
        let current = after
            .version(&key)
            .expect("the current observation loaded every universe group");
        assert_ne!(held, current, "a generation replacement invalidates every held group version");
    }
}

fn assert_answers_equal_the_derivation(
    accelerators: &[&ProductionAccelerator<'_>],
    world: &ReferenceWorld,
) {
    for accelerator in accelerators {
        for image in probe_images(world) {
            assert_eq!(
                accelerator.answer(world, &image),
                Answer::Exact(derive(&world.observation, &image)),
                "a rebuilt accelerator answers the derivation"
            );
        }
    }
}

fn assert_generation_retires_held_handles(
    store: &RawStore,
    held: &[&ProductionAccelerator<'_>],
    prior: &ReceiveRevision,
    world: &ReferenceWorld,
) {
    assert!(
        !current_revision(store).shares_generation(prior),
        "a generation replacement replaces the generation"
    );
    for accelerator in held {
        for answer in answers_at(accelerator, world) {
            assert_eq!(
                answer,
                Answer::Fallback(AcceleratorClass::Foreign),
                "a handle from a replaced generation falls back as foreign"
            );
        }
    }
    assert!(
        store.receive_accelerator().is_none(),
        "a generation replacement leaves no published accelerator"
    );
    let vacant = ProductionAccelerator::new(&**store, Build::Published);
    for answer in answers_at(&vacant, world) {
        assert_eq!(
            answer,
            Answer::Fallback(AcceleratorClass::Absent),
            "the store's accelerator is absent until a rebuild"
        );
    }
}

fn assert_store_revision_replaced(
    current: ReceiveRevision,
    prior: &ReceiveRevision,
    mutation: &str,
) {
    assert!(
        current != *prior && current.shares_generation(prior),
        "{mutation}: the mutation replaces the store revision within the generation"
    );
}

fn assert_stale_fallback(
    held: &ProductionAccelerator<'_>,
    held_answers: &[Answer],
    world: &ReferenceWorld,
    mutation: &str,
) {
    let mut diverged = 0usize;
    for (image, held_answer) in probes().iter().zip(held_answers) {
        let derived = derive(&world.observation, image);
        assert_eq!(
            held.answer(world, image),
            Answer::Fallback(AcceleratorClass::Stale),
            "{mutation}: a held handle falls back as stale"
        );
        assert_eq!(
            read(held, world, &world.observation, image),
            derived,
            "{mutation}: a stale read equals the derivation"
        );
        match *held_answer == Answer::Exact(derived) {
            true => {}
            false => diverged = checked_increment(diverged),
        }
    }
    assert!(
        diverged > 0,
        "{mutation}: the held handle's earlier answers differ, so the fallback is load-bearing"
    );
}

#[derive(Clone, Debug)]
enum TraceStepPlan {
    Put { group: usize, record: RecordPlan },
    Remove { group: usize, index: i32 },
    Install { group: usize, record: RecordPlan },
    Regenerate,
    Clear,
}

fn trace_step_plan() -> impl Strategy<Value = TraceStepPlan> {
    prop_oneof![
        4 => (0..3usize, record_plan())
            .prop_map(|(group, record)| TraceStepPlan::Put { group, record }),
        3 => (0..3usize, 0..4i32).prop_map(|(group, index)| TraceStepPlan::Remove { group, index }),
        2 => (0..3usize, record_plan())
            .prop_map(|(group, record)| TraceStepPlan::Install { group, record }),
        1 => Just(TraceStepPlan::Regenerate),
        1 => Just(TraceStepPlan::Clear),
    ]
}

fn trace_universe() -> [Vec<RawChannel>; 3] {
    [
        raw_channels(&[&[0x01]]),
        raw_channels(&[&[0x01], &[0x00], &[0x01]]),
        raw_channels(&[&[0x00, 0x01]]),
    ]
}

fn trace_group(groups: &[Vec<RawChannel>], index: usize) -> Vec<RawChannel> {
    groups
        .get(index)
        .cloned()
        .expect("a generated group index addresses the universe")
}

fn store_step(
    groups: &[Vec<RawChannel>],
    plan: &TraceStepPlan,
) -> StoreStep<RawChannel, String, String> {
    match plan {
        TraceStepPlan::Put { group, record } => {
            let channels = trace_group(groups, *group);
            StoreStep::Put {
                continuation: planned_continuation(&channels, record),
                channels,
            }
        }
        TraceStepPlan::Remove { group, index } => StoreStep::Remove {
            channels: trace_group(groups, *group),
            index: *index,
        },
        TraceStepPlan::Install { group, record } => {
            let channels = trace_group(groups, *group);
            StoreStep::Install {
                continuation: planned_continuation(&channels, record),
                channels,
            }
        }
        TraceStepPlan::Regenerate => StoreStep::Regenerate,
        TraceStepPlan::Clear => StoreStep::Clear,
    }
}

fn assert_answers_name_present_receives(plans: &[GroupPlan], removals: &[(usize, i32)]) -> usize {
    let store = build_planned_store(plans);
    let universe = planned_universe(plans);
    let lineage = Lineage::new(Vec::new());
    let mut maintained = ProductionAccelerator::new(&*store, Build::Published);
    maintained.rebuild(&lineage.world(hydrated(&store, &universe)));
    let mut removed = 0usize;
    for (group, index) in removals {
        let slot = group
            .checked_rem(universe.len())
            .expect("a planned universe is nonempty");
        let channels = universe
            .get(slot)
            .expect("the reduced group index addresses the universe");
        let before = lineage.world(hydrated(&store, &universe));
        let named = usize::try_from(*index).ok().and_then(|position| {
            before
                .observation
                .ordered_targets(&group_key(channels))
                .get(position)
                .cloned()
        });
        let outcome = store.remove_continuation(channels, *index);
        let after = lineage.world(hydrated(&store, &universe));
        match (outcome, named) {
            (Some(()), Some(target)) => {
                maintained
                    .apply(&MaintenanceOp::RemoveResting(target), &before, &after)
                    .expect("a published accelerator maintains a removal");
                removed = checked_increment(removed);
            }
            (Some(()), None) => panic!("production removed a receive the reference does not order"),
            (None, _) => assert_eq!(after.observation, before.observation),
        }
        for image in probe_images(&after) {
            match maintained.answer(&after, &image) {
                Answer::Exact(occurrences) => {
                    for occurrence in &occurrences {
                        assert!(
                            after.observation.contains_target(&occurrence.target),
                            "an exact answer names only present receives: {occurrence:?}"
                        );
                    }
                    assert_eq!(occurrences, derive(&after.observation, &image));
                }
                Answer::Fallback(class) => {
                    panic!("a maintained accelerator answers exactly; it fell back as {class:?}")
                }
            }
        }
    }
    removed
}

type StaleMutation = (&'static str, fn(&RawStore, &[RawChannel]), bool);

fn insert_stale_receive(store: &RawStore, channels: &[RawChannel]) {
    let inserted = raw_continuation(channels, "stale-insertion", false, BTreeSet::new());
    assert_eq!(store.put_continuation(channels, inserted), Some(true));
}

fn remove_stale_receive(store: &RawStore, channels: &[RawChannel]) {
    assert_eq!(store.remove_continuation(channels, 0), Some(()));
}

fn install_stale_receive(store: &RawStore, channels: &[RawChannel]) {
    let installed = raw_continuation(channels, "stale-install", true, BTreeSet::new());
    assert_eq!(store.install_continuation(channels, installed), Some(()));
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum Pattern {
    #[default]
    Wildcard,
}

impl StableHashSerialize for Pattern {}

impl ColdStoreDecode for Pattern {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

struct WildcardMatch;

impl Match<Pattern, String, String> for WildcardMatch {
    fn get(&self, _pattern: &Pattern, datum: &String) -> Option<String> { Some(datum.clone()) }
}

type StringSpace = RSpace<String, Pattern, String, String>;

async fn string_space() -> StringSpace {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager
        .r_space_stores()
        .await
        .expect("in-memory RSpace stores");
    let matcher: Box<dyn Match<Pattern, String, String>> = Box::new(WildcardMatch);
    RSpace::create(stores, Arc::new(matcher)).expect("an in-memory RSpace")
}

async fn rest_wildcard_receive(space: &StringSpace, channels: &[String], label: &str) {
    let patterns = vec![Pattern::Wildcard; channels.len()];
    let fired = space
        .consume(channels.to_vec(), patterns, label.to_owned(), false, BTreeSet::new())
        .await
        .expect("an in-memory consume succeeds");
    let None = fired else {
        panic!("{label}: no datum is stored, so the receive rests");
    };
}

async fn produce(space: &StringSpace, channel: &str, value: &str) -> Option<String> {
    space
        .produce(channel.to_owned(), value.to_owned(), false)
        .await
        .expect("an in-memory produce succeeds")
        .map(|(fired, _, _)| fired.continuation)
}

fn hydrated_space(space: &StringSpace, universe: &[Vec<String>]) -> Observation {
    observe(&**space.get_store(), universe, true)
}

type StringState = HotStoreState<String, Pattern, String, String>;
type SpaceHandle = ReceiveAcceleratorHandle<String, Pattern, String>;
type SpaceOccurrences = ReceiveOccurrences<String, Pattern, String>;
type SpaceId = ReceiveId<String, Pattern, String>;
type SpaceTarget = ProductionTarget<String, Pattern, String>;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime builds")
}

fn assert_same_hot_state(actual: &StringState, expected: &StringState) {
    assert_eq!(actual.continuations, expected.continuations);
    assert_eq!(actual.installed_continuations, expected.installed_continuations);
    assert_eq!(actual.data, expected.data);
    assert_eq!(actual.joins, expected.joins);
    assert_eq!(actual.installed_joins, expected.installed_joins);
}

fn space_target(group: &GroupKey, target: &SpaceTarget) -> ReceiveTarget {
    ReceiveTarget {
        kind: reference_kind(target.kind()),
        group: group.clone(),
        identity: target.identity_bytes().into_owned(),
        source: target.source_bytes().into_owned(),
    }
}

fn space_group_targets(occurrences: &SpaceOccurrences) -> Vec<(GroupKey, Vec<ReceiveTarget>)> {
    let mut ordered = Vec::with_capacity(occurrences.groups().len());
    for group in occurrences.groups() {
        let key = group_key(group.channels());
        let mut targets = Vec::with_capacity(group.targets().len());
        targets.extend(
            group
                .targets()
                .iter()
                .map(|target| space_target(&key, target)),
        );
        ordered.push((key, targets));
    }
    ordered
}

fn space_occurrences(occurrences: &SpaceOccurrences, image: &[u8]) -> BTreeSet<Occurrence> {
    let mut mapped = BTreeSet::new();
    for group in occurrences.groups() {
        let key = group_key(group.channels());
        let mut positions = Vec::with_capacity(group.positions().len());
        positions.extend(group.positions().iter().map(|position| position.get()));
        for target in group.targets() {
            let occurrence = Occurrence {
                channel: image.to_vec(),
                group: key.clone(),
                positions: positions.clone(),
                target: space_target(&key, target),
            };
            assert!(mapped.insert(occurrence), "a production answer names each occurrence once");
        }
    }
    mapped
}

fn space_ids(occurrences: &SpaceOccurrences) -> Vec<(SpaceId, ReceiveTarget)> {
    let count = occurrences
        .groups()
        .iter()
        .map(|group| group.targets().len())
        .fold(0usize, |total, count| {
            total
                .checked_add(count)
                .expect("an answer's target count fits usize")
        });
    let mut ids = Vec::with_capacity(count);
    for group in occurrences.groups() {
        let key = group_key(group.channels());
        ids.extend(
            group
                .targets()
                .iter()
                .map(|target| (occurrences.receive_id(target), space_target(&key, target))),
        );
    }
    ids
}

fn exact_space_answer(
    space: &StringSpace,
    handle: &SpaceHandle,
    channel: &String,
) -> SpaceOccurrences {
    let store = space.get_store();
    store.get_joins(channel);
    match store.receive_occurrences(Some(handle), channel) {
        ReceiveAnswer::Exact(occurrences) => occurrences,
        ReceiveAnswer::Fallback(class) => {
            panic!(
                "{channel}: a current handle over a hydrated scope answers exactly, not \
                 Fallback({class:?})"
            )
        }
    }
}

fn space_fallback(
    space: &StringSpace,
    handle: &SpaceHandle,
    channel: &String,
) -> Option<ReceiveAcceleratorClass> {
    let store = space.get_store();
    store.get_joins(channel);
    fallback_class(store.receive_occurrences(Some(handle), channel))
}

fn space_groups() -> [Vec<String>; 3] {
    [vec!["space-alpha".to_owned()], vec!["space-alpha".to_owned(), "space-beta".to_owned()], vec![
        "space-beta".to_owned(),
        "space-gamma".to_owned(),
        "space-beta".to_owned(),
    ]]
}

#[derive(Clone, Copy, Debug)]
enum SpaceStepPlan {
    Consume { group: usize, persist: bool },
    Produce { channel: usize },
    Rebuild,
    Checkpoint,
}

fn space_step_plan() -> impl Strategy<Value = SpaceStepPlan> {
    prop_oneof![
        4 => (0..3usize, any::<bool>())
            .prop_map(|(group, persist)| SpaceStepPlan::Consume { group, persist }),
        4 => (0..3usize).prop_map(|channel| SpaceStepPlan::Produce { channel }),
        1 => Just(SpaceStepPlan::Rebuild),
        1 => Just(SpaceStepPlan::Checkpoint),
    ]
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SpaceOutcome {
    Rested,
    Fired {
        continuation: String,
        data: Vec<String>,
    },
    Rebuilt,
    Checkpointed {
        root: Blake2b256Hash,
        log: Log,
    },
}

fn rebuild_when_accelerated(space: &StringSpace, accelerated: bool) {
    match accelerated {
        true => assert_eq!(
            space.get_store().rebuild_receive_accelerator(),
            ReceiveAcceleratorClass::Coherent
        ),
        false => {}
    }
}

async fn apply_space_step(
    space: &StringSpace,
    step: &SpaceStepPlan,
    ordinal: usize,
    accelerated: bool,
) -> SpaceOutcome {
    match step {
        SpaceStepPlan::Consume { group, persist } => {
            let channels = space_groups()
                .get(*group)
                .cloned()
                .expect("a generated group index addresses the space groups");
            let patterns = vec![Pattern::Wildcard; channels.len()];
            let fired = space
                .consume(
                    channels,
                    patterns,
                    format!("space-receive-{ordinal}"),
                    *persist,
                    BTreeSet::new(),
                )
                .await
                .expect("an in-memory consume succeeds");
            match fired {
                Some((result, matched)) => SpaceOutcome::Fired {
                    continuation: result.continuation,
                    data: matched
                        .into_iter()
                        .map(|binding| binding.matched_datum)
                        .collect(),
                },
                None => SpaceOutcome::Rested,
            }
        }
        SpaceStepPlan::Produce { channel } => {
            let name = SPACE_CHANNELS
                .get(*channel)
                .expect("a generated channel index addresses the space channels");
            let fired = space
                .produce((*name).to_owned(), format!("space-value-{ordinal}"), false)
                .await
                .expect("an in-memory produce succeeds");
            match fired {
                Some((result, matched, _)) => SpaceOutcome::Fired {
                    continuation: result.continuation,
                    data: matched
                        .into_iter()
                        .map(|binding| binding.matched_datum)
                        .collect(),
                },
                None => SpaceOutcome::Rested,
            }
        }
        SpaceStepPlan::Rebuild => {
            rebuild_when_accelerated(space, accelerated);
            SpaceOutcome::Rebuilt
        }
        SpaceStepPlan::Checkpoint => {
            let checkpoint = space
                .create_checkpoint()
                .await
                .expect("an in-memory checkpoint succeeds");
            rebuild_when_accelerated(space, accelerated);
            SpaceOutcome::Checkpointed {
                root: checkpoint.root,
                log: checkpoint.log,
            }
        }
    }
}

fn published_group_answers(
    space: &StringSpace,
    groups: &[Vec<String>],
) -> (Observation, BTreeMap<GroupKey, Vec<ReceiveTarget>>) {
    let observation = hydrated_space(space, groups);
    let current = space
        .get_store()
        .receive_accelerator()
        .expect("the maintained space keeps its accelerator published");
    let mut answers: BTreeMap<GroupKey, Vec<ReceiveTarget>> = BTreeMap::new();
    for name in SPACE_CHANNELS {
        let occurrences = exact_space_answer(space, &current, &name.to_owned());
        for (key, targets) in space_group_targets(&occurrences) {
            match answers.insert(key, targets.clone()) {
                Some(previous) => assert_eq!(
                    previous, targets,
                    "a group answers one target list at each of its channels"
                ),
                None => {}
            }
        }
    }
    (observation, answers)
}

fn hydrate_space_scope(space: &StringSpace, groups: &[Vec<String>]) -> Observation {
    let observation = hydrated_space(space, groups);
    let store = space.get_store();
    for name in SPACE_CHANNELS {
        store.get_joins(&name.to_owned());
    }
    observation
}

fn assert_maintained_answers(space: &StringSpace, observation: &Observation, step: &SpaceStepPlan) {
    let current = space
        .get_store()
        .receive_accelerator()
        .expect("the maintained space keeps its accelerator published");
    for name in SPACE_CHANNELS {
        let channel = name.to_owned();
        let image = channel_image(&channel);
        assert_eq!(
            space_occurrences(&exact_space_answer(space, &current, &channel), &image),
            derive(observation, &image),
            "{step:?}: the maintained accelerator answers the derivation"
        );
    }
}

fn assert_same_semantic_identity(
    absent: &StringSpace,
    maintained: &StringSpace,
    step: &SpaceStepPlan,
    phase: &str,
) {
    let absent_store = absent.get_store();
    let maintained_store = maintained.get_store();
    assert_same_hot_state(&maintained_store.snapshot(), &absent_store.snapshot());
    assert!(
        maintained_store
            .pending_data_delta()
            .expect("an in-memory data journal reads") ==
            absent_store
                .pending_data_delta()
                .expect("an in-memory data journal reads"),
        "{step:?} {phase}: the pending data delta is accelerator-free"
    );
    assert!(
        maintained_store
            .pending_continuation_delta()
            .expect("an in-memory continuation journal reads") ==
            absent_store
                .pending_continuation_delta()
                .expect("an in-memory continuation journal reads"),
        "{step:?} {phase}: the pending continuation delta is accelerator-free"
    );
    assert!(
        maintained_store
            .pending_join_delta()
            .expect("an in-memory join journal reads") ==
            absent_store
                .pending_join_delta()
                .expect("an in-memory join journal reads"),
        "{step:?} {phase}: the pending join delta is accelerator-free"
    );
    let maintained_content = maintained_store
        .pending_continuation_content()
        .expect("an in-memory content journal reads")
        .expect("an in-memory store journals continuation content");
    let absent_content = absent_store
        .pending_continuation_content()
        .expect("an in-memory content journal reads")
        .expect("an in-memory store journals continuation content");
    assert_eq!(
        maintained_content
            .iter()
            .map(|(path, transition)| (path, *transition))
            .collect::<Vec<_>>(),
        absent_content
            .iter()
            .map(|(path, transition)| (path, *transition))
            .collect::<Vec<_>>(),
        "{step:?} {phase}: the pending continuation content is accelerator-free"
    );
    let maintained_sources = maintained_store
        .pending_continuation_source_bytes()
        .expect("an in-memory source journal reads")
        .expect("an in-memory store journals continuation sources");
    let absent_sources = absent_store
        .pending_continuation_source_bytes()
        .expect("an in-memory source journal reads")
        .expect("an in-memory store journals continuation sources");
    assert_eq!(
        maintained_sources
            .iter()
            .map(|(path, bytes)| (path, Arc::clone(bytes)))
            .collect::<Vec<_>>(),
        absent_sources
            .iter()
            .map(|(path, bytes)| (path, Arc::clone(bytes)))
            .collect::<Vec<_>>(),
        "{step:?} {phase}: the pending continuation sources are accelerator-free"
    );
    assert_eq!(
        maintained_store
            .checkpoint_changes()
            .expect("an in-memory store plans its checkpoint"),
        absent_store
            .checkpoint_changes()
            .expect("an in-memory store plans its checkpoint"),
        "{step:?} {phase}: the canonical checkpoint plans are accelerator-free"
    );
}

fn assert_exact_groups_are_join_groups_with_targets(
    space: &StringSpace,
    universe: &[Vec<String>],
    channels: &[String],
) {
    let observation = hydrated_space(space, universe);
    let store = space.get_store();
    assert_eq!(store.rebuild_receive_accelerator(), ReceiveAcceleratorClass::Coherent);
    let current = store
        .receive_accelerator()
        .expect("a rebuilt store publishes its accelerator");
    for channel in channels {
        let joined: BTreeSet<GroupKey> = store
            .get_joins(channel)
            .iter()
            .map(|join| group_key(join))
            .filter(|group| !observation.targets(group).is_empty())
            .collect();
        let ReceiveAnswer::Exact(occurrences) = store.receive_occurrences(Some(&current), channel)
        else {
            panic!("{channel}: a hydrated, rebuilt store answers exactly");
        };
        let exact: BTreeSet<GroupKey> = occurrences
            .groups()
            .iter()
            .map(|group| group_key(group.channels()))
            .collect();
        assert_eq!(exact, joined, "{channel}: the exact groups are the join groups with targets");
        let derived: BTreeSet<GroupKey> = channel_groups(&observation, &channel_image(channel))
            .into_iter()
            .map(|(group, _)| group)
            .collect();
        assert_eq!(exact, derived, "{channel}: the exact groups are the derived groups");
    }
}

fn channel_projected_answer(lineage: &Lineage, world: &ReferenceWorld, image: &[u8]) -> Answer {
    let holds = |group: &GroupKey| group.iter().any(|channel| channel.as_slice() == image);
    let mut projected = world.observation.clone();
    projected.store.0.retain(|group, _| holds(group));
    projected.installed.retain(|group, _| holds(group));
    projected.resting.retain(|group, _| holds(group));
    projected.unloaded.retain(|group| holds(group));
    let projected_world = lineage.world(projected);
    let mut reference = ReferenceAccelerator::new();
    reference.rebuild(&projected_world);
    reference.answer(&projected_world, image)
}

fn joins_derivation(
    store: &RawStore,
    observation: &Observation,
    channel: &RawChannel,
) -> BTreeSet<Occurrence> {
    let joins = store.get_joins(channel);
    let mut joined = BTreeSet::new();
    for join in &joins {
        let group = group_key(join);
        assert!(observation.is_loaded(&group), "the joins path reads every join group it names");
        joined.insert(group);
    }
    derive(observation, &channel_image(channel))
        .into_iter()
        .filter(|occurrence| joined.contains(&occurrence.group))
        .collect()
}

fn assert_production_reads_equal_derivation(
    accelerator: &ProductionAccelerator<'_>,
    lineage: &Lineage,
    world: &ReferenceWorld,
    complete: &Observation,
    expected: AcceleratorClass,
) -> usize {
    let mut images = probe_images(world);
    images.extend(complete.channel_images());
    let mut staged = 0usize;
    let mut nonempty = 0usize;
    for image in &images {
        let derived = derive(complete, image);
        let answer = accelerator.answer(world, image);
        let staged_answer = match expected {
            AcceleratorClass::Coherent => Answer::Exact(derived.clone()),
            class => Answer::Fallback(class),
        };
        match expected {
            AcceleratorClass::Incomplete | AcceleratorClass::Coherent => assert_eq!(
                answer,
                channel_projected_answer(lineage, world, image),
                "a current handle answers as the reference over the channel-projected world"
            ),
            _ => assert_eq!(answer, staged_answer, "a vacant or held handle falls back"),
        }
        match answer == staged_answer {
            true => staged = checked_increment(staged),
            false => {}
        }
        assert_eq!(
            read(accelerator, world, complete, image),
            derived,
            "every accelerator state reads the derivation"
        );
        match derived.is_empty() {
            true => {}
            false => nonempty = checked_increment(nonempty),
        }
    }
    match expected {
        AcceleratorClass::Incomplete => assert!(staged > 0, "some probe falls back as incomplete"),
        _ => assert_eq!(staged, images.len(), "every probe answers in the {expected:?} state"),
    }
    nonempty
}

#[derive(Clone, Copy, Debug)]
enum ScopeGap {
    UnloadedJoins,
    UnloadedJoinGroup,
    UnloadedInstalledGroup,
}

fn scope_gap_store() -> (RawStore, [Vec<RawChannel>; 3]) {
    let resident = raw_channels(&[&[0x01]]);
    let historical = raw_channels(&[&[0x00], &[0x01, 0x00]]);
    let installed = raw_channels(&[&[0x01, 0x00, 0xFF]]);
    let mut history = StubHistory::default();
    history.put_continuations(historical.clone(), vec![
        raw_continuation(&historical, "gap-history-first", false, BTreeSet::new()),
        raw_continuation(&historical, "gap-history-second", true, BTreeSet::from([1])),
    ]);
    for channel in &historical {
        history.put_join(channel.clone(), historical.clone());
    }
    let store = raw_hot_store_with_history(RawState::default(), history);
    let resident_record = raw_continuation(&resident, "gap-resident", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&resident, resident_record), Some(true));
    for channel in &resident {
        assert_eq!(store.put_join(channel, &resident), Some(()));
    }
    let installed_record = raw_continuation(&installed, "gap-installed", true, BTreeSet::new());
    assert_eq!(store.install_continuation(&installed, installed_record), Some(()));
    (store, [resident, historical, installed])
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

#[tokio::test(flavor = "current_thread")]
async fn production_exact_groups_are_the_join_groups_with_targets() {
    let space = string_space().await;
    let left = "join-left".to_owned();
    let right = "join-right".to_owned();
    let other = "join-other".to_owned();
    let pair = vec![left.clone(), other.clone()];
    let repeated = vec![left.clone(), left.clone(), other.clone()];
    let single = vec![right.clone()];
    let installed = vec![other.clone()];
    rest_wildcard_receive(&space, &pair, "pair").await;
    rest_wildcard_receive(&space, &repeated, "repeated").await;
    rest_wildcard_receive(&space, &single, "single").await;
    let installation = space
        .install(installed.clone(), vec![Pattern::Wildcard], "installed".to_owned())
        .await
        .expect("an in-memory install succeeds");
    assert!(installation.is_none(), "no datum matches the installed receive");
    let universe = vec![pair, repeated, single, installed];
    let channels = [left, right.clone(), other];
    assert_exact_groups_are_join_groups_with_targets(&space, &universe, &channels);
    assert_eq!(produce(&space, &right, "fire").await, Some("single".to_owned()));
    assert_exact_groups_are_join_groups_with_targets(&space, &universe, &channels);
    assert!(
        space.get_store().get_joins(&right).is_empty(),
        "a fired single receive leaves no join"
    );
}

#[test]
fn production_history_groups_without_joins_are_invisible_to_both_paths() {
    let resident = raw_channels(&[&[0x01]]);
    let unjoined = raw_channels(&[&[0x00], &[0x01, 0x00]]);
    let mut history = StubHistory::default();
    history.put_continuations(unjoined.clone(), vec![
        raw_continuation(&unjoined, "unjoined-first", false, BTreeSet::new()),
        raw_continuation(&unjoined, "unjoined-second", true, BTreeSet::from([0])),
    ]);
    let store = raw_hot_store_with_history(RawState::default(), history);
    let resident_record = raw_continuation(&resident, "unjoined-resident", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&resident, resident_record), Some(true));
    for channel in &resident {
        assert_eq!(store.put_join(channel, &resident), Some(()));
    }
    let universe = vec![resident.clone(), unjoined.clone()];
    let lineage = Lineage::new(Vec::new());
    let unloaded = lineage.world(observe(&*store, &universe, false));
    assert_eq!(unloaded.observation.unloaded, BTreeSet::from([group_key(&unjoined)]));
    let blind = BUILDS.map(|build| rebuilt(&store, build, &unloaded));
    for image in probe_images(&unloaded) {
        let joined = joins_derivation(&store, &unloaded.observation, &RawChannel(image.clone()));
        for accelerator in &blind {
            assert_eq!(
                accelerator.answer(&unloaded, &image),
                Answer::Exact(joined.clone()),
                "an unjoined history group is invisible to the accelerator and to the joins"
            );
        }
    }
    let revision = current_revision(&store);
    assert_eq!(store.get_continuations(&unjoined).len(), 2, "an explicit load reads the group");
    assert!(current_revision(&store) == revision, "a history load issues no new revision");
    let loaded = lineage.world(observe(&*store, &universe, false));
    let sighted = BUILDS.map(|build| rebuilt(&store, build, &loaded));
    let complete = hydrated(&store, &universe);
    let mut widened = BTreeSet::new();
    for image in probe_images(&loaded) {
        let derived = derive(&complete, &image);
        let joined = joins_derivation(&store, &complete, &RawChannel(image.clone()));
        assert!(joined.is_subset(&derived), "the joins derivation is part of the occurrences");
        for accelerator in &sighted {
            assert_eq!(
                accelerator.answer(&loaded, &image),
                Answer::Exact(derived.clone()),
                "after an explicit load the accelerator answers every occurrence"
            );
        }
        match joined == derived {
            true => {}
            false => {
                widened.insert(image);
            }
        }
    }
    let unjoined_images: BTreeSet<ChannelImage> = unjoined.iter().map(channel_image).collect();
    assert_eq!(
        widened, unjoined_images,
        "only the unjoined group's channels answer beyond the joins derivation"
    );
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

#[test]
fn production_insertions_maintain_the_published_accelerator() {
    let strategy = proptest::collection::vec((0..3usize, record_plan()), 1..=8);
    check_property(STORE_CASES, &strategy, |insertions| {
        let store = raw_hot_store();
        let groups = fixed_groups();
        let lineage = Lineage::new(Vec::new());
        let mut maintained = ProductionAccelerator::new(&*store, Build::Published);
        maintained.rebuild(&lineage.world(hydrated(&store, &groups)));
        let mut inserted_count = 0usize;
        for (group_index, plan) in &insertions {
            let channels = groups
                .get(*group_index)
                .expect("a generated group index addresses the fixed groups");
            let continuation = planned_continuation(channels, plan);
            let key = group_key(channels);
            let before = lineage.world(hydrated(&store, &groups));
            let inserted = store
                .put_continuation(channels, continuation.clone())
                .expect("an in-memory put reports whether it inserted");
            let after = lineage.world(hydrated(&store, &groups));
            match inserted {
                true => {
                    prop_assert_ne!(
                        before.observation.version(&key),
                        after.observation.version(&key),
                        "an insertion replaces the group version"
                    );
                    for answer in answers_at(&maintained, &after) {
                        prop_assert_eq!(
                            answer,
                            Answer::Fallback(AcceleratorClass::Stale),
                            "a handle from before an insertion never answers"
                        );
                    }
                    inserted_count = checked_increment(inserted_count);
                }
                false => prop_assert_eq!(&after, &before, "a duplicate put keeps the world"),
            }
            maintained
                .apply(
                    &MaintenanceOp::PutResting(expected_target(
                        ReceiveKind::Resting,
                        channels,
                        &continuation,
                    )),
                    &before,
                    &after,
                )
                .expect("a published accelerator maintains an insertion");
            assert_maintained_equals_rebuild(&store, &maintained, &after);
        }
        prop_assert!(inserted_count > 0, "every generated trace inserts at least once");
        Ok(())
    });
}

#[test]
fn production_nonfinal_removals_maintain_the_published_accelerator() {
    for index in 0..3usize {
        let store = raw_hot_store();
        let channels = raw_channels(&[&[0x01], &[0x00], &[0x01]]);
        let universe = vec![channels.clone()];
        let key = group_key(&channels);
        for body in ["removal-first", "removal-second", "removal-third"] {
            let record = raw_continuation(&channels, body, false, BTreeSet::new());
            assert_eq!(store.put_continuation(&channels, record), Some(true));
        }
        let lineage = Lineage::new(Vec::new());
        let before = lineage.world(hydrated(&store, &universe));
        let mut maintained = ProductionAccelerator::new(&*store, Build::Published);
        maintained.rebuild(&before);
        let named = before
            .observation
            .ordered_targets(&key)
            .get(index)
            .cloned()
            .expect("the group orders three receives");
        let position = i32::try_from(index).expect("a removal index fits i32");
        assert_eq!(store.remove_continuation(&channels, position), Some(()));
        let after = lineage.world(hydrated(&store, &universe));
        assert_ne!(
            before.observation.version(&key),
            after.observation.version(&key),
            "a removal replaces the group version"
        );
        for answer in answers_at(&maintained, &after) {
            assert_eq!(
                answer,
                Answer::Fallback(AcceleratorClass::Stale),
                "a handle from before a removal never answers"
            );
        }
        assert!(!after.observation.contains_target(&named));
        assert_eq!(after.observation.targets(&key).len(), 2);
        maintained
            .apply(&MaintenanceOp::RemoveResting(named), &before, &after)
            .expect("a published accelerator maintains a removal");
        assert_maintained_equals_rebuild(&store, &maintained, &after);
    }
}

#[test]
fn production_final_removal_clears_the_maintained_group() {
    let store = raw_hot_store();
    let emptied = raw_channels(&[&[0x01]]);
    let neighbour = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![emptied.clone(), neighbour.clone()];
    let emptied_key = group_key(&emptied);
    let neighbour_key = group_key(&neighbour);
    let only = raw_continuation(&emptied, "final-only", false, BTreeSet::new());
    let neighbour_record = raw_continuation(&neighbour, "final-neighbour", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&emptied, only), Some(true));
    assert_eq!(store.put_continuation(&neighbour, neighbour_record), Some(true));
    let lineage = Lineage::new(Vec::new());
    let before = lineage.world(hydrated(&store, &universe));
    let mut maintained = ProductionAccelerator::new(&*store, Build::Published);
    maintained.rebuild(&before);
    let named = before
        .observation
        .ordered_targets(&emptied_key)
        .first()
        .cloned()
        .expect("the emptied group holds one receive");
    assert_eq!(store.remove_continuation(&emptied, 0), Some(()));
    let after = lineage.world(hydrated(&store, &universe));
    maintained
        .apply(&MaintenanceOp::RemoveResting(named), &before, &after)
        .expect("a published accelerator maintains a final removal");
    assert_maintained_equals_rebuild(&store, &maintained, &after);
    assert!(!after.observation.support().contains(&emptied_key));
    for image in probes() {
        assert!(
            ordered_targets(&exact_production(&maintained, &image))
                .iter()
                .all(|(group, _)| *group != emptied_key),
            "a cleared group leaves no occurrence group, not even an empty one"
        );
    }
    assert_eq!(
        after.observation.targets(&neighbour_key),
        before.observation.targets(&neighbour_key)
    );
    assert_eq!(
        after.observation.version(&neighbour_key),
        before.observation.version(&neighbour_key),
        "a group sharing a channel keeps its version"
    );
    let shared = ordered_targets(&exact_production(&maintained, &[0x01]));
    assert_eq!(shared.len(), 1, "only the neighbour answers at the shared channel");
    assert!(shared.iter().all(|(group, _)| *group == neighbour_key));
}

#[test]
fn production_installs_maintain_and_set_state_retires_the_published_accelerator() {
    let store = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![channels.clone()];
    let key = group_key(&channels);
    let twin = raw_continuation(&channels, "install-twin", true, BTreeSet::new());
    assert_eq!(store.put_continuation(&channels, twin.clone()), Some(true));
    let mut lineage = Lineage::new(Vec::new());
    let resting_only = lineage.world(hydrated(&store, &universe));
    let mut maintained = ProductionAccelerator::new(&*store, Build::Published);
    maintained.rebuild(&resting_only);
    assert_eq!(store.install_continuation(&channels, twin.clone()), Some(()));
    lineage.reinstall();
    let installed = lineage.world(hydrated(&store, &universe));
    assert_ne!(
        resting_only.observation.version(&key),
        installed.observation.version(&key),
        "an install replaces the group version"
    );
    for answer in answers_at(&maintained, &installed) {
        assert_eq!(
            answer,
            Answer::Fallback(AcceleratorClass::Stale),
            "a handle from before an install never answers"
        );
    }
    assert_eq!(
        installed.observation.targets(&key),
        BTreeSet::from([
            expected_target(ReceiveKind::Resting, &channels, &twin),
            expected_target(ReceiveKind::Installed, &channels, &twin),
        ]),
        "a byte-identical installed twin keeps a distinct target"
    );
    maintained
        .apply(
            &MaintenanceOp::Install(expected_target(ReceiveKind::Installed, &channels, &twin)),
            &resting_only,
            &installed,
        )
        .expect("a published accelerator maintains an install");
    assert_maintained_equals_rebuild(&store, &maintained, &installed);
    for image in [&[0x01][..], &[0x00][..]] {
        assert_eq!(exact(maintained.answer(&installed, image)).len(), 2);
    }
    let replacement = raw_continuation(&channels, "install-replacement", true, BTreeSet::new());
    assert_eq!(store.install_continuation(&channels, replacement.clone()), Some(()));
    lineage.reinstall();
    let reinstalled = lineage.world(hydrated(&store, &universe));
    assert_ne!(installed.observation.version(&key), reinstalled.observation.version(&key));
    maintained
        .apply(
            &MaintenanceOp::Install(expected_target(
                ReceiveKind::Installed,
                &channels,
                &replacement,
            )),
            &installed,
            &reinstalled,
        )
        .expect("a published accelerator maintains a reinstall");
    assert_maintained_equals_rebuild(&store, &maintained, &reinstalled);
    assert_eq!(
        reinstalled.observation.targets(&key),
        BTreeSet::from([
            expected_target(ReceiveKind::Resting, &channels, &twin),
            expected_target(ReceiveKind::Installed, &channels, &replacement),
        ])
    );
    let mut uninstalled_state = store.snapshot();
    uninstalled_state.installed_continuations.clear();
    store.set_state(uninstalled_state);
    lineage.regenerate();
    let uninstalled = lineage.world(hydrated(&store, &universe));
    for answer in answers_at(&maintained, &uninstalled) {
        assert_eq!(
            answer,
            Answer::Fallback(AcceleratorClass::Foreign),
            "set_state retires the maintained handle"
        );
    }
    maintained.rebuild(&uninstalled);
    assert_maintained_equals_rebuild(&store, &maintained, &uninstalled);
    assert_eq!(
        uninstalled.observation.targets(&key),
        BTreeSet::from([expected_target(ReceiveKind::Resting, &channels, &twin)])
    );
}

#[test]
fn production_history_loads_extend_the_maintained_scope() {
    let resident = raw_channels(&[&[0x01]]);
    let first_history = raw_channels(&[&[0x00]]);
    let second_history = raw_channels(&[&[0x01, 0x00], &[0x01]]);
    let historical_first = raw_continuation(&first_history, "load-first", false, BTreeSet::new());
    let mut history = StubHistory::default();
    history.put_continuations(first_history.clone(), vec![
        historical_first.clone(),
        raw_continuation(&first_history, "load-second", true, BTreeSet::from([0])),
    ]);
    history.put_continuations(second_history.clone(), vec![raw_continuation(
        &second_history,
        "load-third",
        false,
        BTreeSet::new(),
    )]);
    for group in [&first_history, &second_history] {
        for channel in group {
            history.put_join(channel.clone(), group.clone());
        }
    }
    let store = raw_hot_store_with_history(RawState::default(), history);
    let resident_record = raw_continuation(&resident, "load-resident", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&resident, resident_record.clone()), Some(true));
    for channel in &resident {
        assert_eq!(store.put_join(channel, &resident), Some(()));
    }
    let universe = vec![resident.clone(), first_history.clone(), second_history.clone()];
    let resident_key = group_key(&resident);
    let lineage = Lineage::new(Vec::new());
    let partial = lineage.world(observe(&*store, &universe, false));
    assert_eq!(
        partial.observation.unloaded,
        BTreeSet::from([group_key(&first_history), group_key(&second_history)])
    );
    let mut maintained = ProductionAccelerator::new(&*store, Build::Published);
    maintained.rebuild(&partial);
    assert_maintained_equals_rebuild(&store, &maintained, &partial);
    let mut before = partial;
    for channels in [&first_history, &second_history] {
        let probe = channels
            .first()
            .expect("a history group has a first channel")
            .0
            .clone();
        assert_eq!(
            maintained.answer(&before, &probe),
            Answer::Fallback(AcceleratorClass::Incomplete),
            "a channel joined to an unloaded history group falls back"
        );
        let loaded = store.get_continuations(channels);
        let after = lineage.world(observe(&*store, &universe, false));
        assert_eq!(
            before.observation.version(&resident_key),
            after.observation.version(&resident_key),
            "a history load keeps every resident version"
        );
        let operation = MaintenanceOp::LoadHistory {
            group: group_key(channels),
            targets: loaded
                .iter()
                .map(|continuation| expected_target(ReceiveKind::Resting, channels, continuation))
                .collect(),
        };
        maintained
            .apply(&operation, &before, &after)
            .expect("a published accelerator maintains a history load");
        assert_maintained_equals_rebuild(&store, &maintained, &after);
        let extended = derive(&after.observation, &probe);
        assert!(!extended.is_empty(), "the loaded group holds receives at its first channel");
        assert_eq!(
            maintained.answer(&after, &probe),
            Answer::Exact(extended),
            "a history load extends the maintained scope"
        );
        before = after;
    }
    assert!(before.observation.is_complete());
    let neutral = answers_at(&maintained, &before);
    let revision = store.receive_revision();
    let duplicates: [(&Vec<RawChannel>, StringContinuation); 3] = [
        (&resident, resident_record.clone()),
        (&resident, alias_of(&resident_record)),
        (&first_history, historical_first),
    ];
    for (channels, continuation) in duplicates {
        assert_eq!(store.put_continuation(channels, continuation), Some(false));
        assert!(store.receive_revision() == revision, "a duplicate put keeps the receive revision");
        assert_eq!(answers_at(&maintained, &before), neutral, "a duplicate put keeps every answer");
    }
}

#[test]
fn production_clear_and_set_state_retire_held_handles_and_rebuilds_equal_the_derivation() {
    let seeded = raw_channels(&[&[0x01]]);
    let historical = raw_channels(&[&[0x00]]);
    let bumped = raw_channels(&[&[0x01, 0x00]]);
    let mut history = StubHistory::default();
    history.put_continuations(historical.clone(), vec![raw_continuation(
        &historical,
        "generation-history",
        false,
        BTreeSet::new(),
    )]);
    for channel in &historical {
        history.put_join(channel.clone(), historical.clone());
    }
    let mut state = RawState::default();
    state
        .continuations
        .insert(seeded.clone(), vec![raw_continuation(
            &seeded,
            "generation-seeded",
            false,
            BTreeSet::new(),
        )]);
    let store = raw_hot_store_with_history(state, history);
    let bumped_record = raw_continuation(&bumped, "generation-bumped", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&bumped, bumped_record), Some(true));
    let universe = vec![seeded.clone(), historical.clone(), bumped.clone()];
    let mut lineage = Lineage::new(Vec::new());
    let original = lineage.world(hydrated(&store, &universe));
    let original_published = rebuilt(&store, Build::Published, &original);
    let original_detached = rebuilt(&store, Build::Detached, &original);
    assert_answers_equal_the_derivation(&[&original_published, &original_detached], &original);
    let original_revision = current_revision(&store);
    let original_snapshot = store.snapshot();
    store.clear();
    lineage.regenerate();
    let cleared = lineage.world(hydrated(&store, &universe));
    assert_every_version_replaced(&original.observation, &cleared.observation, &universe);
    assert_generation_retires_held_handles(
        &store,
        &[&original_published, &original_detached],
        &original_revision,
        &cleared,
    );
    let cleared_published = rebuilt(&store, Build::Published, &cleared);
    let cleared_detached = rebuilt(&store, Build::Detached, &cleared);
    assert_answers_equal_the_derivation(&[&cleared_published, &cleared_detached], &cleared);
    let cleared_revision = current_revision(&store);
    store.set_state(original_snapshot);
    lineage.regenerate();
    let restored = lineage.world(hydrated(&store, &universe));
    assert_every_version_replaced(&cleared.observation, &restored.observation, &universe);
    assert_every_version_replaced(&original.observation, &restored.observation, &universe);
    assert_eq!(derive_all(&restored.observation), derive_all(&original.observation));
    assert_generation_retires_held_handles(
        &store,
        &[&original_published, &cleared_published, &cleared_detached],
        &cleared_revision,
        &restored,
    );
    let restored_published = rebuilt(&store, Build::Published, &restored);
    let restored_detached = rebuilt(&store, Build::Detached, &restored);
    assert_answers_equal_the_derivation(&[&restored_published, &restored_detached], &restored);
    let restored_revision = current_revision(&store);
    store.set_state(store.snapshot());
    lineage.regenerate();
    let replaced = lineage.world(hydrated(&store, &universe));
    assert_every_version_replaced(&restored.observation, &replaced.observation, &universe);
    assert_eq!(derive_all(&replaced.observation), derive_all(&restored.observation));
    assert_generation_retires_held_handles(
        &store,
        &[&restored_published, &restored_detached],
        &restored_revision,
        &replaced,
    );
    let replaced_published = rebuilt(&store, Build::Published, &replaced);
    let replaced_detached = rebuilt(&store, Build::Detached, &replaced);
    assert_answers_equal_the_derivation(&[&replaced_published, &replaced_detached], &replaced);
}

#[test]
fn production_equal_group_versions_imply_equal_group_answers() {
    let prefix = [
        SpaceStepPlan::Consume {
            group: 1,
            persist: true,
        },
        SpaceStepPlan::Consume {
            group: 0,
            persist: false,
        },
        SpaceStepPlan::Produce { channel: 0 },
    ];
    let strategy = proptest::collection::vec(space_step_plan(), 0..=10);
    check_property(TRACE_CASES, &strategy, |suffix| {
        runtime().block_on(async {
            let space = string_space().await;
            let groups = space_groups();
            let keys: Vec<GroupKey> = groups.iter().map(|channels| group_key(channels)).collect();
            let steps = prefix
                .len()
                .checked_add(suffix.len())
                .expect("the step count fits usize");
            let capacity = steps
                .checked_add(1)
                .expect("the observation count fits usize");
            let mut observed: Vec<(Observation, BTreeMap<GroupKey, Vec<ReceiveTarget>>)> =
                Vec::with_capacity(capacity);
            let mut outcomes: Vec<SpaceOutcome> = Vec::with_capacity(steps);
            assert_eq!(
                space.get_store().rebuild_receive_accelerator(),
                ReceiveAcceleratorClass::Coherent
            );
            observed.push(published_group_answers(&space, &groups));
            for (ordinal, step) in prefix.iter().chain(suffix.iter()).enumerate() {
                outcomes.push(apply_space_step(&space, step, ordinal, true).await);
                observed.push(published_group_answers(&space, &groups));
            }
            assert_eq!(
                outcomes.get(2),
                Some(&SpaceOutcome::Fired {
                    continuation: "space-receive-1".to_owned(),
                    data: vec!["space-value-2".to_owned()],
                }),
                "the prefix produce fires the single-channel receive"
            );
            let (
                [single_key, joined_key, _],
                Some((rested, rested_answers)),
                Some((fired, fired_answers)),
            ) = (keys.as_slice(), observed.get(2), observed.get(3))
            else {
                panic!("the fixed prefix is observed");
            };
            assert_eq!(
                rested_answers.get(single_key).map(Vec::len),
                Some(1),
                "the prefix receive rests"
            );
            assert_eq!(fired_answers.get(single_key), None, "the prefix produce fires it");
            assert_eq!(
                rested.version(joined_key),
                fired.version(joined_key),
                "the produce leaves the joined group's version"
            );
            assert_eq!(
                fired_answers.get(joined_key).map(Vec::len),
                Some(1),
                "the joined group still answers its receive"
            );
            let mut compared = 0usize;
            for key in &keys {
                for (left, left_answers) in &observed {
                    for (right, right_answers) in &observed {
                        match (left.version(key), right.version(key)) {
                            (Some(left_version), Some(right_version))
                                if left_version == right_version =>
                            {
                                assert_eq!(
                                    left_answers.get(key),
                                    right_answers.get(key),
                                    "equal group versions imply equal production group answers"
                                );
                                match left_answers.get(key) {
                                    Some(_) => compared = checked_increment(compared),
                                    None => {}
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            assert!(compared > 0, "an equal-version pair answers a nonempty group");
        });
        Ok(())
    });
}

#[test]
fn production_reads_equal_derivation_in_every_accelerator_state() {
    let resident = raw_channels(&[&[0x01]]);
    let historical = raw_channels(&[&[0x01], &[0x00]]);
    let mut history = StubHistory::default();
    history.put_continuations(historical.clone(), vec![raw_continuation(
        &historical,
        "state-history",
        false,
        BTreeSet::new(),
    )]);
    for channel in &historical {
        history.put_join(channel.clone(), historical.clone());
    }
    let store = raw_hot_store_with_history(RawState::default(), history);
    let resident_record = raw_continuation(&resident, "state-resident", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&resident, resident_record), Some(true));
    for channel in &resident {
        assert_eq!(store.put_join(channel, &resident), Some(()));
    }
    let universe = vec![resident.clone(), historical.clone()];
    let mut lineage = Lineage::new(Vec::new());
    let partial = lineage.world(observe(&*store, &universe, false));
    let incomplete = rebuilt(&store, Build::Published, &partial);
    let complete = lineage.world(hydrated(&store, &universe));
    let mut nonempty = assert_production_reads_equal_derivation(
        &incomplete,
        &lineage,
        &partial,
        &complete.observation,
        AcceleratorClass::Incomplete,
    );
    let absent = ProductionAccelerator::new(&*store, Build::Published);
    let absent_nonempty = assert_production_reads_equal_derivation(
        &absent,
        &lineage,
        &complete,
        &complete.observation,
        AcceleratorClass::Absent,
    );
    assert!(absent_nonempty > 0, "the absent state is probed where the derivation is nonempty");
    nonempty = nonempty
        .checked_add(absent_nonempty)
        .expect("the probe tally fits usize");
    let coherent = rebuilt(&store, Build::Published, &complete);
    let _ = assert_production_reads_equal_derivation(
        &coherent,
        &lineage,
        &complete,
        &complete.observation,
        AcceleratorClass::Coherent,
    );
    let stale_record = raw_continuation(&resident, "state-stale", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&resident, stale_record), Some(true));
    let stale_world = lineage.world(hydrated(&store, &universe));
    let _ = assert_production_reads_equal_derivation(
        &coherent,
        &lineage,
        &stale_world,
        &stale_world.observation,
        AcceleratorClass::Stale,
    );
    store.set_state(store.snapshot());
    lineage.regenerate();
    let foreign_world = lineage.world(hydrated(&store, &universe));
    let _ = assert_production_reads_equal_derivation(
        &coherent,
        &lineage,
        &foreign_world,
        &foreign_world.observation,
        AcceleratorClass::Foreign,
    );
    assert!(nonempty > 0);
}

#[test]
fn production_refused_updates_leave_the_store_and_accelerator_unchanged() {
    let store = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![channels.clone()];
    let installed = raw_continuation(&channels, "failure-installed", true, BTreeSet::new());
    let resting = raw_continuation(&channels, "failure-resting", false, BTreeSet::new());
    assert_eq!(store.install_continuation(&channels, installed), Some(()));
    assert_eq!(store.put_continuation(&channels, resting), Some(true));
    let lineage = Lineage::new(Vec::new());
    let before = lineage.world(hydrated(&store, &universe));
    let mut maintained = ProductionAccelerator::new(&*store, Build::Published);
    maintained.rebuild(&before);
    let answers = answers_at(&maintained, &before);
    let revision = store.receive_revision();
    for index in [0, 2, 7, -1] {
        assert_eq!(store.remove_continuation(&channels, index), None, "index {index} is refused");
        assert_eq!(
            hydrated(&store, &universe),
            before.observation,
            "a refused removal changes no version or target"
        );
        assert!(
            store.receive_revision() == revision,
            "a refused removal keeps the receive revision"
        );
        assert_eq!(
            answers_at(&maintained, &before),
            answers,
            "a held handle answers unchanged after a refused removal"
        );
        assert_eq!(
            maintained.artifact_revisions(),
            vec![before.revision.clone()],
            "the published accelerator stays at the current revision"
        );
        assert_eq!(
            answers_at(&rebuilt(&store, Build::Detached, &before), &before),
            answers,
            "a fresh build answers as the held handle"
        );
    }
    let inserted = raw_continuation(&channels, "failure-inserted", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&channels, inserted.clone()), Some(true));
    let after = lineage.world(hydrated(&store, &universe));
    let operation =
        MaintenanceOp::PutResting(expected_target(ReceiveKind::Resting, &channels, &inserted));
    maintained
        .apply(&operation, &before, &after)
        .expect("a published accelerator maintains an insertion");
    assert_maintained_equals_rebuild(&store, &maintained, &after);
    let maintained_answers = answers_at(&maintained, &after);
    assert_eq!(
        maintained.apply(&operation, &before, &after),
        Err(Fault::Unmaintainable(AcceleratorClass::Stale)),
        "an update from a superseded base is refused"
    );
    assert_eq!(
        answers_at(&maintained, &after),
        maintained_answers,
        "a refused update leaves the maintained handle unchanged"
    );
    assert_eq!(maintained.artifact_revisions(), vec![after.revision.clone()]);
}

#[test]
fn production_index_and_catalog_publish_at_one_revision() {
    let resident = raw_channels(&[&[0x01], &[0x01, 0x00]]);
    let historical = raw_channels(&[&[0x00]]);
    let seeded = raw_hot_store();
    for body in ["stamp-first", "stamp-second"] {
        let record = raw_continuation(&resident, body, false, BTreeSet::new());
        assert_eq!(seeded.put_continuation(&resident, record), Some(true));
    }
    for channel in &resident {
        assert_eq!(seeded.put_join(channel, &resident), Some(()));
    }
    let mut history = StubHistory::default();
    history.put_continuations(historical.clone(), vec![raw_continuation(
        &historical,
        "stamp-history",
        false,
        BTreeSet::new(),
    )]);
    for channel in &historical {
        history.put_join(channel.clone(), historical.clone());
    }
    let store = raw_hot_store_with_history(seeded.snapshot(), history);
    let universe = vec![resident.clone(), historical.clone()];
    let resident_key = group_key(&resident);
    let mut lineage = Lineage::new(Vec::new());
    let initial = lineage.world(observe(&*store, &universe, false));
    let mut accelerator = rebuilt(&store, Build::Published, &initial);
    assert_one_revision(&store, &accelerator, &initial);

    let inserted = raw_continuation(&resident, "stamp-third", false, BTreeSet::new());
    let before = lineage.world(observe(&*store, &universe, false));
    assert_eq!(store.put_continuation(&resident, inserted.clone()), Some(true));
    let after = lineage.world(observe(&*store, &universe, false));
    accelerator
        .apply(
            &MaintenanceOp::PutResting(expected_target(ReceiveKind::Resting, &resident, &inserted)),
            &before,
            &after,
        )
        .expect("an insertion is maintained");
    assert_one_revision(&store, &accelerator, &after);

    let installed = raw_continuation(&resident, "stamp-installed", true, BTreeSet::new());
    let before = lineage.world(observe(&*store, &universe, false));
    assert_eq!(store.install_continuation(&resident, installed.clone()), Some(()));
    lineage.reinstall();
    let after = lineage.world(observe(&*store, &universe, false));
    accelerator
        .apply(
            &MaintenanceOp::Install(expected_target(ReceiveKind::Installed, &resident, &installed)),
            &before,
            &after,
        )
        .expect("an install is maintained");
    assert_one_revision(&store, &accelerator, &after);

    let before = lineage.world(observe(&*store, &universe, false));
    let named = before
        .observation
        .ordered_targets(&resident_key)
        .get(1)
        .cloned()
        .expect("the resident group orders an installed and three resting receives");
    assert_eq!(store.remove_continuation(&resident, 1), Some(()));
    let after = lineage.world(observe(&*store, &universe, false));
    accelerator
        .apply(&MaintenanceOp::RemoveResting(named), &before, &after)
        .expect("a removal is maintained");
    assert_one_revision(&store, &accelerator, &after);

    let before = lineage.world(observe(&*store, &universe, false));
    let loaded = store.get_continuations(&historical);
    let after = lineage.world(observe(&*store, &universe, false));
    let mut targets = Vec::with_capacity(loaded.len());
    targets.extend(
        loaded
            .iter()
            .map(|continuation| expected_target(ReceiveKind::Resting, &historical, continuation)),
    );
    let operation = MaintenanceOp::LoadHistory {
        group: group_key(&historical),
        targets,
    };
    accelerator
        .apply(&operation, &before, &after)
        .expect("a history load is maintained");
    assert_one_revision(&store, &accelerator, &after);
    assert!(after.observation.is_complete(), "the history load completes the store");
    assert_maintained_equals_rebuild(&store, &accelerator, &after);
}

#[tokio::test(flavor = "current_thread")]
async fn production_same_count_group_replacement_never_reuses_a_stale_answer_or_handle() {
    let space = string_space().await;
    let channel = "replaced-group".to_owned();
    let single = vec![channel.clone()];
    let joined = vec![channel.clone(), "replaced-partner".to_owned()];
    let universe = vec![single.clone(), joined.clone()];
    let single_key = group_key(&single);
    let image = channel_image(&channel);
    rest_wildcard_receive(&space, &single, "replaced-first").await;
    rest_wildcard_receive(&space, &joined, "replaced-joined").await;
    let store = space.get_store();
    let cached = hydrated_space(&space, &universe);
    assert_eq!(store.rebuild_receive_accelerator(), ReceiveAcceleratorClass::Coherent);
    let held = store
        .receive_accelerator()
        .expect("a rebuilt store publishes its accelerator");
    let held_occurrences = exact_space_answer(&space, &held, &channel);
    let held_answer = space_occurrences(&held_occurrences, &image);
    assert_eq!(held_answer, derive(&cached, &image), "the held handle answers the derivation");
    let held_ids = space_ids(&held_occurrences);
    assert_eq!(produce(&space, &channel, "replacing").await, Some("replaced-first".to_owned()));
    rest_wildcard_receive(&space, &single, "replaced-second").await;
    let replaced = hydrated_space(&space, &universe);
    assert_eq!(
        cached.targets(&single_key).len(),
        replaced.targets(&single_key).len(),
        "the replacement keeps the candidate count"
    );
    assert_ne!(cached.targets(&single_key), replaced.targets(&single_key));
    assert_ne!(cached.version(&single_key), replaced.version(&single_key));
    assert_eq!(
        space_fallback(&space, &held, &channel),
        Some(ReceiveAcceleratorClass::Stale),
        "a handle held across the replacement never answers"
    );
    let mut surviving = 0usize;
    for (id, target) in &held_ids {
        assert!(
            store.resolve_receive(id).is_none(),
            "an id held across the replacement never resolves"
        );
        match replaced.contains_target(target) {
            true => surviving = checked_increment(surviving),
            false => {}
        }
    }
    assert!(surviving > 0, "a held id names a receive that survives the replacement");
    let current = store
        .receive_accelerator()
        .expect("the maintained store keeps its accelerator published");
    let current_occurrences = exact_space_answer(&space, &current, &channel);
    let current_answer = space_occurrences(&current_occurrences, &image);
    assert_eq!(
        current_answer,
        derive(&replaced, &image),
        "the maintained handle answers the replacement"
    );
    assert_eq!(current_answer.len(), held_answer.len(), "the replacement keeps the answer's size");
    assert_ne!(current_answer, held_answer, "the maintained handle never reuses the held answer");
    let detached = store
        .detached_receive_accelerator()
        .expect("a fresh build indexes the store");
    assert_eq!(
        space_occurrences(&exact_space_answer(&space, &detached, &channel), &image),
        current_answer,
        "the maintained answer equals a fresh build's"
    );
    for (id, _) in space_ids(&current_occurrences) {
        assert!(store.resolve_receive(&id).is_some(), "a current id resolves");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn production_stale_tickets_ids_and_handles_never_commit() {
    let space = string_space().await;
    let channel = "stale-ticket".to_owned();
    let channels = vec![channel.clone()];
    let universe = vec![channels.clone()];
    let image = channel_image(&channel);
    assert_eq!(produce(&space, &channel, "datum").await, None);
    let store = space.get_store();
    let held_continuation = WaitingContinuation::create(
        &channels,
        &vec![Pattern::Wildcard],
        &"held".to_owned(),
        false,
        BTreeSet::new(),
    );
    assert_eq!(store.put_continuation(&channels, held_continuation), Some(true));
    assert_eq!(store.put_join(&channel, &channels), Some(()));
    assert_eq!(store.rebuild_receive_accelerator(), ReceiveAcceleratorClass::Coherent);
    let old_handle = store
        .receive_accelerator()
        .expect("a rebuilt store publishes its accelerator");
    let old_ids = space_ids(&exact_space_answer(&space, &old_handle, &channel));
    assert_eq!(old_ids.len(), 1, "the old handle names the held receive");
    for (id, _) in &old_ids {
        assert!(store.resolve_receive(id).is_some(), "an id resolves before the mutation");
    }
    let held = space
        .enabled_rendezvous()
        .into_iter()
        .next()
        .expect("the resting datum enables the held receive");
    let new_head = WaitingContinuation::create(
        &channels,
        &vec![Pattern::Wildcard],
        &"new-head".to_owned(),
        false,
        BTreeSet::new(),
    );
    assert_eq!(store.put_continuation(&channels, new_head), Some(true));
    let expected = store.snapshot();
    let _ = space.take_event_log().await;
    assert!(space.process_match_found(held).is_none(), "production refuses a stale ticket");
    assert_same_hot_state(&store.snapshot(), &expected);
    assert!(space.take_event_log().await.is_empty(), "a stale ticket publishes no COMM");
    let current = hydrated_space(&space, &universe);
    for (id, target) in &old_ids {
        assert!(
            current.contains_target(target),
            "the old id names a receive that is still present"
        );
        assert!(
            store.resolve_receive(id).is_none(),
            "an id from before the mutation never resolves"
        );
    }
    assert_eq!(
        space_fallback(&space, &old_handle, &channel),
        Some(ReceiveAcceleratorClass::Stale),
        "the old handle falls back as stale"
    );
    let fresh_handle = store
        .receive_accelerator()
        .expect("the maintained store keeps its accelerator published");
    let fresh_occurrences = exact_space_answer(&space, &fresh_handle, &channel);
    assert_eq!(
        space_occurrences(&fresh_occurrences, &image),
        derive(&current, &image),
        "the fresh handle answers the derivation"
    );
    let fresh_ids = space_ids(&fresh_occurrences);
    assert_eq!(fresh_ids.len(), 2, "the fresh handle names both receives");
    for (id, _) in &fresh_ids {
        assert!(store.resolve_receive(id).is_some(), "a fresh id resolves");
    }
    let fresh_ticket = space
        .enabled_rendezvous()
        .into_iter()
        .next()
        .expect("the resting datum still enables a receive");
    assert!(space.process_match_found(fresh_ticket).is_some(), "a fresh ticket commits");
    assert!(
        matches!(space.take_event_log().await.as_slice(), [Event::Comm(_)]),
        "the fresh ticket publishes exactly one COMM"
    );
}

#[test]
fn production_accelerator_never_changes_semantic_identity() {
    let prefix = [
        SpaceStepPlan::Consume {
            group: 0,
            persist: false,
        },
        SpaceStepPlan::Rebuild,
        SpaceStepPlan::Produce { channel: 1 },
        SpaceStepPlan::Produce { channel: 0 },
        SpaceStepPlan::Checkpoint,
    ];
    let strategy = proptest::collection::vec(space_step_plan(), 0..=12);
    check_property(TRACE_CASES, &strategy, |suffix| {
        runtime().block_on(async {
            let absent = string_space().await;
            let maintained = string_space().await;
            assert_eq!(
                maintained.get_store().rebuild_receive_accelerator(),
                ReceiveAcceleratorClass::Coherent
            );
            let groups = space_groups();
            let mut fired = 0usize;
            let mut checkpoints = 0usize;
            for (ordinal, step) in prefix.iter().chain(suffix.iter()).enumerate() {
                let absent_outcome = apply_space_step(&absent, step, ordinal, false).await;
                let maintained_outcome = apply_space_step(&maintained, step, ordinal, true).await;
                assert_eq!(
                    maintained_outcome, absent_outcome,
                    "{step:?}: the accelerator never changes a selection or a checkpoint"
                );
                match maintained_outcome {
                    SpaceOutcome::Fired { .. } => fired = checked_increment(fired),
                    SpaceOutcome::Checkpointed { .. } => {
                        checkpoints = checked_increment(checkpoints)
                    }
                    SpaceOutcome::Rested | SpaceOutcome::Rebuilt => {}
                }
                assert_same_semantic_identity(&absent, &maintained, step, "after the step");
                let absent_observation = hydrate_space_scope(&absent, &groups);
                let maintained_observation = hydrate_space_scope(&maintained, &groups);
                assert_eq!(
                    derive_all(&maintained_observation),
                    derive_all(&absent_observation),
                    "{step:?}: both spaces derive the same relation"
                );
                assert_maintained_answers(&maintained, &maintained_observation, step);
                assert_same_semantic_identity(&absent, &maintained, step, "after hydration");
            }
            assert!(fired > 0, "the trace exercises COMMs");
            assert!(checkpoints > 0, "the trace exercises checkpoints");
        });
        Ok(())
    });
}

#[test]
fn production_maintenance_equals_rebuild_over_generated_traces() {
    let strategy = proptest::collection::vec(trace_step_plan(), 1..=12);
    check_property(TRACE_CASES, &strategy, |plans| {
        let groups = trace_universe();
        let [_, _, historical] = &groups;
        let mut history = StubHistory::default();
        history.put_continuations(historical.clone(), vec![
            raw_continuation(historical, "trace-history-first", false, BTreeSet::new()),
            raw_continuation(historical, "trace-history-second", true, BTreeSet::from([0])),
        ]);
        for channel in historical {
            history.put_join(channel.clone(), historical.clone());
        }
        let store = raw_hot_store_with_history(RawState::default(), history);
        let steps: Vec<StoreStep<RawChannel, String, String>> =
            plans.iter().map(|plan| store_step(&groups, plan)).collect();
        let published = Cell::new(true);
        let make = || {
            let build = match published.replace(false) {
                true => Build::Published,
                false => Build::Detached,
            };
            ProductionAccelerator::new(&*store, build)
        };
        let report = check_trace(make, &*store, &groups, &probes(), &steps);
        prop_assert_eq!(report.steps, steps.len());
        Ok(())
    });
}

#[test]
fn production_answers_never_name_absent_receives() {
    let fixed = vec![GroupPlan {
        images: vec![2, 1],
        records: vec![
            RecordPlan {
                body: 7,
                persist: false,
                peek_first: false,
            },
            RecordPlan {
                body: 8,
                persist: true,
                peek_first: false,
            },
        ],
        installed: None,
    }];
    assert!(
        assert_answers_name_present_receives(&fixed, &[(0, 0), (0, 0)]) > 0,
        "the fixed trace removes receives"
    );
    let strategy = (store_plan(), proptest::collection::vec((0..4usize, 0..4i32), 1..=6));
    check_property(STORE_CASES, &strategy, |(plans, removals)| {
        let _ = assert_answers_name_present_receives(&plans, &removals);
        Ok(())
    });
}

#[tokio::test(flavor = "current_thread")]
async fn production_held_handles_fall_back_after_every_mutation() {
    let mutations: [StaleMutation; 3] = [
        ("insertion", insert_stale_receive, false),
        ("removal", remove_stale_receive, false),
        ("install", install_stale_receive, true),
    ];
    for (name, mutate, reinstalls) in mutations {
        let store = raw_hot_store();
        let channels = raw_channels(&[&[0x01], &[0x00]]);
        let universe = vec![channels.clone()];
        for body in ["stale-first", "stale-second"] {
            let record = raw_continuation(&channels, body, false, BTreeSet::new());
            assert_eq!(store.put_continuation(&channels, record), Some(true));
        }
        let mut lineage = Lineage::new(Vec::new());
        let before = lineage.world(hydrated(&store, &universe));
        let held = rebuilt(&store, Build::Published, &before);
        let held_answers = answers_at(&held, &before);
        let revision = current_revision(&store);
        mutate(&store, &channels);
        if reinstalls {
            lineage.reinstall();
        }
        let after = lineage.world(hydrated(&store, &universe));
        assert_store_revision_replaced(current_revision(&store), &revision, name);
        assert_stale_fallback(&held, &held_answers, &after, name);
    }
    let space = string_space().await;
    let channel = "stale-comm".to_owned();
    let channels = vec![channel.clone()];
    let universe = vec![channels.clone()];
    let image = channel_image(&channel);
    rest_wildcard_receive(&space, &channels, "stale-comm-receive").await;
    let store = space.get_store();
    assert_eq!(derive(&hydrated_space(&space, &universe), &image).len(), 1);
    assert_eq!(store.rebuild_receive_accelerator(), ReceiveAcceleratorClass::Coherent);
    let held = store
        .receive_accelerator()
        .expect("a rebuilt store publishes its accelerator");
    let revision = held.revision().clone();
    store.get_joins(&channel);
    let ReceiveAnswer::Exact(occurrences) = store.receive_occurrences(Some(&held), &channel) else {
        panic!("comm removal: a current handle answers exactly");
    };
    match occurrences.groups() {
        [group] => {
            assert_eq!(group.targets().len(), 1, "comm removal: the held handle names the receive")
        }
        _ => panic!("comm removal: exactly one group holds the channel"),
    }
    assert_eq!(produce(&space, "stale-comm", "fired").await, Some("stale-comm-receive".to_owned()));
    assert!(
        derive(&hydrated_space(&space, &universe), &image).is_empty(),
        "comm removal: the COMM consumes the only receive"
    );
    assert_store_revision_replaced(
        store
            .receive_revision()
            .expect("an in-memory store has a receive revision"),
        &revision,
        "comm removal",
    );
    store.get_joins(&channel);
    assert_eq!(
        fallback_class(store.receive_occurrences(Some(&held), &channel)),
        Some(ReceiveAcceleratorClass::Stale),
        "comm removal: a held handle falls back as stale"
    );
}

#[test]
fn production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated() {
    let gaps =
        [ScopeGap::UnloadedJoins, ScopeGap::UnloadedJoinGroup, ScopeGap::UnloadedInstalledGroup];
    for gap in gaps {
        let (store, universe) = scope_gap_store();
        let [_, historical, installed] = &universe;
        let gap_group = match gap {
            ScopeGap::UnloadedJoins | ScopeGap::UnloadedJoinGroup => historical,
            ScopeGap::UnloadedInstalledGroup => installed,
        };
        let channel = gap_group
            .first()
            .cloned()
            .expect("a gap group has a first channel");
        match gap {
            ScopeGap::UnloadedJoins => {}
            ScopeGap::UnloadedJoinGroup => {
                assert_eq!(store.get_joins(&channel), vec![historical.clone()]);
            }
            ScopeGap::UnloadedInstalledGroup => {
                assert!(store.get_joins(&channel).is_empty(), "the install wrote no join");
            }
        }
        assert_eq!(store.rebuild_receive_accelerator(), ReceiveAcceleratorClass::Coherent);
        let lineage = Lineage::new(Vec::new());
        let image = channel_image(&channel);
        let unhydrated = lineage.world(observe(&*store, &universe, false));
        assert_eq!(
            channel_projected_answer(&lineage, &unhydrated, &image),
            Answer::Fallback(AcceleratorClass::Incomplete),
            "{gap:?}: the channel-projected world is incomplete"
        );
        let revision = current_revision(&store);
        let current = store
            .receive_accelerator()
            .expect("a rebuilt store publishes its accelerator");
        assert_eq!(
            fallback_class(store.receive_occurrences(Some(&current), &channel)),
            Some(ReceiveAcceleratorClass::Incomplete),
            "{gap:?}: a current handle falls back as incomplete"
        );
        store.hydrate_receive_scope(&channel);
        assert!(current_revision(&store) == revision, "{gap:?}: hydration issues no new revision");
        let hydrated_world = lineage.world(observe(&*store, &universe, false));
        let fresh = BUILDS.map(|build| rebuilt(&store, build, &hydrated_world));
        let derived = derive(&hydrated(&store, &universe), &image);
        assert!(!derived.is_empty(), "{gap:?}: the gap group holds receives at the channel");
        assert_eq!(
            channel_projected_answer(&lineage, &hydrated_world, &image),
            Answer::Exact(derived.clone()),
            "{gap:?}: hydration completes the channel-projected world"
        );
        for accelerator in &fresh {
            assert_eq!(
                accelerator.answer(&hydrated_world, &image),
                Answer::Exact(derived.clone()),
                "{gap:?}: hydration completes the channel"
            );
        }
    }
}

#[test]
fn production_held_handles_never_answer_for_groups_loaded_after_they_were_taken() {
    let resident = raw_channels(&[&[0x01]]);
    let first_history = raw_channels(&[&[0x00]]);
    let second_history = raw_channels(&[&[0x01, 0x00], &[0x01]]);
    let mut history = StubHistory::default();
    history.put_continuations(first_history.clone(), vec![raw_continuation(
        &first_history,
        "held-first",
        false,
        BTreeSet::new(),
    )]);
    history.put_continuations(second_history.clone(), vec![raw_continuation(
        &second_history,
        "held-second",
        true,
        BTreeSet::from([0]),
    )]);
    for group in [&first_history, &second_history] {
        for channel in group {
            history.put_join(channel.clone(), group.clone());
        }
    }
    let store = raw_hot_store_with_history(RawState::default(), history);
    let resident_record = raw_continuation(&resident, "held-resident", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&resident, resident_record), Some(true));
    for channel in &resident {
        assert_eq!(store.put_join(channel, &resident), Some(()));
    }
    let universe = vec![resident.clone(), first_history.clone(), second_history.clone()];
    let lineage = Lineage::new(Vec::new());
    let taken = lineage.world(observe(&*store, &universe, false));
    let held = rebuilt(&store, Build::Published, &taken);
    let loads = [&first_history, &second_history];
    let mut loaded: Vec<GroupKey> = Vec::with_capacity(loads.len());
    for channels in loads {
        let channel = channels
            .first()
            .expect("a history group has a first channel");
        store.hydrate_receive_scope(channel);
        assert_eq!(
            held.artifact_revisions(),
            vec![taken.revision.clone()],
            "a history load issues no new revision, so the held handle stays current"
        );
        loaded.push(group_key(channels));
        let world = lineage.world(observe(&*store, &universe, false));
        let fresh = BUILDS.map(|build| rebuilt(&store, build, &world));
        let mut withheld = 0usize;
        for image in probe_images(&world) {
            let expected = channel_projected_answer(&lineage, &world, &image);
            for accelerator in &fresh {
                assert_eq!(
                    accelerator.answer(&world, &image),
                    expected,
                    "a handle taken after the load answers the channel-projected world"
                );
            }
            let later = loaded
                .iter()
                .any(|group| group.iter().any(|member| *member == image));
            let held_answer = held.answer(&world, &image);
            match later {
                true => assert_eq!(
                    held_answer,
                    Answer::Fallback(AcceleratorClass::Incomplete),
                    "a held handle never answers for a group loaded after it was taken"
                ),
                false => assert_eq!(
                    held_answer, expected,
                    "a held handle answers unchanged where no later load reaches"
                ),
            }
            let withholds =
                later && matches!(&expected, Answer::Exact(occurrences) if !occurrences.is_empty());
            match withholds {
                true => withheld = checked_increment(withheld),
                false => {}
            }
        }
        assert!(withheld > 0, "the held handle withholds an answer the fresh handles give");
    }
}
