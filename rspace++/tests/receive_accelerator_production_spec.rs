use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed, TestRunner};
use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::ordered_receive::{PreparedContinuationRecord, RestingReceiveVisit};
use rspace_plus_plus::rspace::rho_path::ReceiveChannelQuery;

#[path = "support/receive_occurrence_reference.rs"]
#[allow(dead_code)]
mod reference;

#[path = "support/receive_accelerator_production.rs"]
mod production;

use production::{
    Build, ProductionAccelerator, RawChannel, RawStore, StringContinuation, raw_hot_store,
};
use reference::{
    AcceleratorUnderTest, Answer, ChannelImage, GroupKey, Lineage, Observation, Occurrence,
    ReceiveKind, ReceiveTarget, ReferenceWorld, capacity, derive, observe, occurrences_of,
};

const IMAGES: [&[u8]; 5] = [&[], &[0x00], &[0x01], &[0x01, 0x00], &[0x01, 0x00, 0xFF]];
const ABSENT_IMAGE: &[u8] = &[0x01, 0x01];
const BUILDS: [Build; 2] = [Build::Published, Build::Detached];
const STORE_CASES: u32 = 256;
const PROPERTY_SEED: u64 = 41_318_011;

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

fn build_planned_store(plans: &[GroupPlan]) -> RawStore {
    let store = raw_hot_store();
    for plan in plans {
        let channels = indexed_channels(&plan.images);
        for record in &plan.records {
            let outcome =
                store.put_continuation(&channels, planned_continuation(&channels, record));
            assert!(outcome.is_some(), "an in-memory put reports whether it inserted");
        }
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
