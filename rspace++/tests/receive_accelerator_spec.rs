use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed, TestRunner};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::history::history_reader::HistoryReaderBase;
use rspace_plus_plus::rspace::hot_store::{HotStore, HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::metrics_constants::RETAINED_AUTOMATON_CACHE_HIT_METRIC;
use rspace_plus_plus::rspace::ordered_receive::PreparedContinuationRecord;
use rspace_plus_plus::rspace::rho_path::RhoPathEncode;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};

#[path = "support/receive_occurrence_reference.rs"]
#[allow(dead_code)]
mod reference;

use reference::adoption::Adoption;
use reference::{
    AcceleratorClass, AcceleratorUnderTest, Answer, BucketedTargets, BuiltAccelerator,
    ChannelImage, CommitOutcome, Fault, FaultStage, GroupKey, Lineage, MaintenanceOp, Observation,
    Occurrence, ReceiveId, ReceiveKind, ReceiveTarget, ReferenceAccelerator, ReferenceWorld,
    StoreStep, capacity, channel_groups, channel_image, check_trace, commit_candidate, derive,
    derive_all, group_key, mint_ids, observe, occurrences_of, read, resolve,
};

const IMAGES: [&[u8]; 5] = [&[], &[0x00], &[0x01], &[0x01, 0x00], &[0x01, 0x00, 0xFF]];
const ABSENT_IMAGE: &[u8] = &[0x01, 0x01];
const STORE_CASES: u32 = 256;
const TRACE_CASES: u32 = 64;
const PROPERTY_SEED: u64 = 41_318_010;
const TRACE_CHANNELS: [&str; 3] = ["trace-alpha", "trace-beta", "trace-gamma"];

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize)]
struct RawChannel(Vec<u8>);

impl StableHashSerialize for RawChannel {}

impl RhoPathEncode for RawChannel {
    fn rho_path_structure(&self) -> Vec<u8> { self.0.clone() }

    fn rho_path_identity(&self) -> Vec<u8> { self.0.clone() }
}

impl ColdStoreDecode for RawChannel {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        Ok((RawChannel(bytes.to_vec()), bytes.len()))
    }
}

type StringContinuation = WaitingContinuation<String, String>;
type RawStore = Box<dyn HotStore<RawChannel, String, String, String>>;

#[derive(Clone, Default)]
struct StubHistory {
    continuations: Arc<Mutex<HashMap<Vec<RawChannel>, Vec<StringContinuation>>>>,
    continuation_reads: Arc<AtomicUsize>,
}

impl StubHistory {
    fn put_continuations(&self, channels: Vec<RawChannel>, continuations: Vec<StringContinuation>) {
        self.continuations
            .lock()
            .expect("stub history lock")
            .insert(channels, continuations);
    }

    fn continuation_reads(&self) -> usize { self.continuation_reads.load(Ordering::SeqCst) }
}

impl HistoryReaderBase<RawChannel, String, String, String> for StubHistory {
    fn get_data_canonical(&self, _channel: &RawChannel) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(&self, channels: &[RawChannel]) -> Vec<StringContinuation> {
        self.continuation_reads.fetch_add(1, Ordering::SeqCst);
        self.continuations
            .lock()
            .expect("stub history lock")
            .get(channels)
            .cloned()
            .unwrap_or_default()
    }

    fn get_joins_canonical(&self, _channel: &RawChannel) -> Vec<Vec<RawChannel>> { Vec::new() }
}

fn raw_hot_store() -> (StubHistory, RawStore) {
    let history = StubHistory::default();
    let hot_store = HotStoreInstances::create_from_hs_and_hr(
        HotStoreState::default(),
        Box::new(history.clone()),
    );
    (history, hot_store)
}

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

fn probe_images(world: &ReferenceWorld) -> BTreeSet<ChannelImage> {
    let mut images = world.observation.channel_images();
    images.extend(probes());
    images
}

fn hydrated(store: &RawStore, universe: &[Vec<RawChannel>]) -> Observation {
    observe(&**store, universe, true)
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

fn flip_last(bytes: &[u8]) -> Vec<u8> {
    let mut flipped = bytes.to_vec();
    match flipped.last_mut() {
        Some(last) => *last ^= 0x01,
        None => flipped.push(0x01),
    }
    flipped
}

fn perturbations(target: &ReceiveTarget) -> Vec<ReceiveTarget> {
    let mut truncated_identity = target.identity.clone();
    truncated_identity.pop();
    let mut extended_source = target.source.clone();
    extended_source.push(0x00);
    let mut extended_group = target.group.clone();
    extended_group.push(ABSENT_IMAGE.to_vec());
    let swapped_kind = match target.kind {
        ReceiveKind::Installed => ReceiveKind::Resting,
        ReceiveKind::Resting => ReceiveKind::Installed,
    };
    vec![
        ReceiveTarget {
            identity: flip_last(&target.identity),
            ..target.clone()
        },
        ReceiveTarget {
            source: flip_last(&target.source),
            ..target.clone()
        },
        ReceiveTarget {
            identity: truncated_identity,
            ..target.clone()
        },
        ReceiveTarget {
            source: extended_source,
            ..target.clone()
        },
        ReceiveTarget {
            group: extended_group,
            ..target.clone()
        },
        ReceiveTarget {
            kind: swapped_kind,
            ..target.clone()
        },
    ]
}

fn all_ids(world: &ReferenceWorld) -> Vec<ReceiveId> {
    let mut ids = Vec::new();
    for image in probe_images(world) {
        ids.extend(mint_ids(&world.revision, &derive(&world.observation, &image)));
    }
    ids
}

fn assert_matches_rebuild(maintained: &ReferenceAccelerator, world: &ReferenceWorld) {
    assert_eq!(
        maintained.classify(world),
        AcceleratorClass::Coherent,
        "a maintained accelerator is coherent with the world it was maintained to"
    );
    let built = maintained
        .built()
        .expect("a maintained accelerator is built");
    assert_eq!(
        built,
        &BuiltAccelerator::build(world),
        "incremental maintenance equals a rebuild from the derived relation"
    );
    for image in probe_images(world) {
        assert_eq!(
            maintained.answer(world, &image),
            Answer::Exact(derive(&world.observation, &image)),
            "a coherent accelerator answers the derivation"
        );
    }
}

fn assert_single_revision(accelerator: &ReferenceAccelerator, world: &ReferenceWorld) {
    let revisions = accelerator.artifact_revisions();
    assert_eq!(revisions.len(), 3, "the catalog, occurrence index and automaton are published");
    for revision in &revisions {
        assert_eq!(
            revision, &world.revision,
            "every artifact is published at the current revision"
        );
    }
    let built = accelerator
        .built()
        .expect("a published accelerator is built");
    assert_eq!(built.revision, world.revision, "the accelerator revision equals its artifacts");
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

fn assert_reads_equal_derivation(
    accelerator: &ReferenceAccelerator,
    world: &ReferenceWorld,
    complete: &Observation,
    expected: AcceleratorClass,
) -> usize {
    assert_eq!(accelerator.classify(world), expected, "the accelerator is in the staged state");
    let mut images = probe_images(world);
    images.extend(complete.channel_images());
    let mut nonempty = 0usize;
    for image in &images {
        let derived = derive(complete, image);
        let answer = accelerator.answer(world, image);
        match expected {
            AcceleratorClass::Coherent => assert_eq!(answer, Answer::Exact(derived.clone())),
            other => assert_eq!(answer, Answer::Fallback(other)),
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
    nonempty
}

fn assert_stale_fallback(
    accelerator: &ReferenceAccelerator,
    world: &ReferenceWorld,
    mutation: &str,
) {
    assert_eq!(
        accelerator.classify(world),
        AcceleratorClass::Stale,
        "{mutation}: the held accelerator is stale"
    );
    let mut diverged = 0usize;
    for image in probe_images(world) {
        let derived = derive(&world.observation, &image);
        assert_eq!(
            accelerator.answer(world, &image),
            Answer::Fallback(AcceleratorClass::Stale),
            "{mutation}: a stale accelerator falls back"
        );
        assert_eq!(
            read(accelerator, world, &world.observation, &image),
            derived,
            "{mutation}: a stale read equals the derivation"
        );
        match accelerator.lookup(&image) == derived {
            true => {}
            false => diverged = checked_increment(diverged),
        }
    }
    assert!(
        diverged > 0,
        "{mutation}: the stale lookup differs from the derivation, so the fallback is load-bearing"
    );
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

fn build_store_in_order(plans: &[GroupPlan], reverse_puts: bool) -> (StubHistory, RawStore) {
    let (history, store) = raw_hot_store();
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
    (history, store)
}

fn build_planned_store(plans: &[GroupPlan]) -> (StubHistory, RawStore) {
    build_store_in_order(plans, false)
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

fn fixed_groups() -> [Vec<RawChannel>; 3] {
    [
        raw_channels(&[&[0x01]]),
        raw_channels(&[&[0x01], &[0x00], &[0x01]]),
        raw_channels(&[&[], &[0x01, 0x00]]),
    ]
}

fn assert_construction_order_invariance(plans: &[GroupPlan]) {
    let universe = planned_universe(plans);
    let (_forward_history, forward) = build_store_in_order(plans, false);
    let (_reverse_history, reverse) = build_store_in_order(plans, true);
    let forward_observation = hydrated(&forward, &universe);
    let reverse_observation = hydrated(&reverse, &universe);
    assert_eq!(
        derive_all(&forward_observation),
        derive_all(&reverse_observation),
        "the derived relation is independent of construction order"
    );
    let forward_world = Lineage::new(Vec::new()).world(forward_observation.clone());
    let reverse_world = Lineage::new(Vec::new()).world(reverse_observation.clone());
    for image in planned_probes(plans) {
        let forward_targets: Vec<ReceiveTarget> =
            mint_ids(&forward_world.revision, &derive(&forward_observation, &image))
                .into_iter()
                .map(|id| id.target)
                .collect();
        let reverse_targets: Vec<ReceiveTarget> =
            mint_ids(&reverse_world.revision, &derive(&reverse_observation, &image))
                .into_iter()
                .map(|id| id.target)
                .collect();
        assert_eq!(
            forward_targets, reverse_targets,
            "minted ids name the same targets in both orders"
        );
    }
    for channels in &universe {
        assert_ordered_targets_follow_production(&forward, &forward_observation, channels);
        assert_ordered_targets_follow_production(&reverse, &reverse_observation, channels);
    }
}

fn assert_ordered_targets_follow_production(
    store: &RawStore,
    observation: &Observation,
    channels: &[RawChannel],
) {
    let key = group_key(channels);
    let installed = observation.installed.contains_key(&key);
    let production: Vec<ReceiveTarget> = store
        .get_continuations(channels)
        .iter()
        .enumerate()
        .map(|(index, continuation)| {
            let kind = match (installed, index) {
                (true, 0) => ReceiveKind::Installed,
                _ => ReceiveKind::Resting,
            };
            expected_target(kind, channels, continuation)
        })
        .collect();
    assert_eq!(
        observation.ordered_targets(&key),
        production,
        "the ordered lookup equals the production historical order"
    );
}

fn constant_hash(_target: &ReceiveTarget) -> u64 { 0 }

fn identity_prefix_hash(target: &ReceiveTarget) -> u64 {
    target.identity.first().copied().map_or(0, u64::from)
}

fn resting_orders(observation: &Observation, key: &GroupKey) -> BTreeSet<u64> {
    observation
        .resting
        .get(key)
        .map(|records| records.iter().map(|record| record.order).collect())
        .unwrap_or_default()
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
    let (_history, store) = build_planned_store(plans);
    let universe = planned_universe(plans);
    let lineage = Lineage::new(Vec::new());
    let mut accelerator = ReferenceAccelerator::new();
    accelerator.rebuild(&lineage.world(hydrated(&store, &universe)));
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
                accelerator
                    .apply(&MaintenanceOp::RemoveResting(target), &before, &after)
                    .expect("a coherent accelerator maintains a removal");
                removed = checked_increment(removed);
            }
            (Some(()), None) => panic!("production removed a receive the reference does not order"),
            (None, _) => assert_eq!(after.observation, before.observation),
        }
        for image in probe_images(&after) {
            match accelerator.answer(&after, &image) {
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum Pattern {
    #[default]
    Wildcard,
    StringMatch(String),
}

impl StableHashSerialize for Pattern {}

impl ColdStoreDecode for Pattern {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

#[derive(Clone, Copy)]
struct PlainMatch;

impl Match<Pattern, String, String> for PlainMatch {
    fn get(&self, pattern: &Pattern, datum: &String) -> Option<String> {
        match pattern {
            Pattern::Wildcard => Some(datum.clone()),
            Pattern::StringMatch(value) if value == datum => Some(datum.clone()),
            Pattern::StringMatch(_) => None,
        }
    }
}

#[derive(Clone, Copy)]
struct RetainedPathMatch;

impl Match<Pattern, String, String> for RetainedPathMatch {
    fn get(&self, pattern: &Pattern, datum: &String) -> Option<String> {
        PlainMatch.get(pattern, datum)
    }

    fn retained_pattern_path(&self, pattern: &Pattern) -> Option<Vec<u8>> {
        match pattern {
            Pattern::Wildcard => None,
            Pattern::StringMatch(value) => Some(value.as_bytes().to_vec()),
        }
    }

    fn retained_datum_path(&self, datum: &String) -> Option<Vec<u8>> {
        Some(datum.as_bytes().to_vec())
    }

    fn supports_rho_path_candidate_pruning(&self) -> bool { true }
}

type StringSpace = RSpace<String, Pattern, String, String>;
type StringState = HotStoreState<String, Pattern, String, String>;

async fn string_space(matcher: Box<dyn Match<Pattern, String, String>>) -> StringSpace {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager
        .r_space_stores()
        .await
        .expect("in-memory RSpace stores");
    RSpace::create(stores, Arc::new(matcher)).expect("an in-memory RSpace")
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime builds")
}

async fn rest_receive(
    space: &StringSpace,
    channels: &[String],
    patterns: Vec<Pattern>,
    label: &str,
    persist: bool,
) {
    let fired = space
        .consume(channels.to_vec(), patterns, label.to_owned(), persist, BTreeSet::new())
        .await
        .expect("an in-memory consume succeeds");
    let None = fired else {
        panic!("{label}: no datum matches, so the receive rests");
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

fn retained_cache_hits(snapshotter: &Snapshotter) -> u64 {
    let mut hits = 0u64;
    for (key, _, _, value) in snapshotter.snapshot().into_vec() {
        if let (true, DebugValue::Counter(count)) =
            (key.key().name() == RETAINED_AUTOMATON_CACHE_HIT_METRIC, value)
        {
            hits = hits
                .checked_add(count)
                .expect("the cache hit count fits u64");
        }
    }
    hits
}

fn assert_same_hot_state(actual: &StringState, expected: &StringState) {
    assert_eq!(actual.continuations, expected.continuations);
    assert_eq!(actual.installed_continuations, expected.installed_continuations);
    assert_eq!(actual.data, expected.data);
    assert_eq!(actual.joins, expected.joins);
    assert_eq!(actual.installed_joins, expected.installed_joins);
}

fn assert_join_projection(space: &StringSpace, universe: &[Vec<String>], channels: &[String]) {
    let store = space.get_store();
    let observation = observe(&**store, universe, true);
    for channel in channels {
        let projected: BTreeSet<GroupKey> = store
            .get_joins(channel)
            .iter()
            .map(|join| group_key(join))
            .collect();
        let derived: BTreeSet<GroupKey> = channel_groups(&observation, &channel_image(channel))
            .into_iter()
            .map(|(group, _)| group)
            .collect();
        assert_eq!(projected, derived, "{channel}: the join bag projects onto the derived groups");
    }
}

#[derive(Clone, Copy, Debug)]
enum SpaceStepPlan {
    Consume {
        group: usize,
        persist: bool,
        pattern: usize,
    },
    Produce {
        channel: usize,
        value: usize,
    },
}

fn space_step_plan() -> impl Strategy<Value = SpaceStepPlan> {
    prop_oneof![
        (0..3usize, any::<bool>(), 0..3usize).prop_map(|(group, persist, pattern)| {
            SpaceStepPlan::Consume {
                group,
                persist,
                pattern,
            }
        }),
        (0..3usize, 0..3usize)
            .prop_map(|(channel, value)| SpaceStepPlan::Produce { channel, value }),
    ]
}

fn trace_groups() -> [Vec<String>; 3] {
    [vec!["trace-alpha".to_owned()], vec!["trace-alpha".to_owned(), "trace-beta".to_owned()], vec![
        "trace-beta".to_owned(),
        "trace-gamma".to_owned(),
        "trace-beta".to_owned(),
    ]]
}

fn trace_pattern(index: usize) -> Pattern {
    match index {
        0 => Pattern::Wildcard,
        other => Pattern::StringMatch(format!("value-{other}")),
    }
}

async fn apply_space_step(
    space: &StringSpace,
    groups: &[Vec<String>],
    step: &SpaceStepPlan,
    ordinal: usize,
) {
    match step {
        SpaceStepPlan::Consume {
            group,
            persist,
            pattern,
        } => {
            let channels = groups
                .get(*group)
                .cloned()
                .expect("a generated group index addresses the trace groups");
            let patterns = vec![trace_pattern(*pattern); channels.len()];
            let _ = space
                .consume(
                    channels,
                    patterns,
                    format!("trace-receive-{ordinal}"),
                    *persist,
                    BTreeSet::new(),
                )
                .await
                .expect("an in-memory consume succeeds");
        }
        SpaceStepPlan::Produce { channel, value } => {
            let name = TRACE_CHANNELS
                .get(*channel)
                .expect("a generated channel index addresses the trace channels");
            let _ = space
                .produce((*name).to_owned(), format!("value-{value}"), false)
                .await
                .expect("an in-memory produce succeeds");
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ScriptStep {
    Consume {
        channels: &'static [&'static str],
        patterns: &'static [&'static str],
        label: &'static str,
        persist: bool,
    },
    Produce {
        channel: &'static str,
        value: &'static str,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ScriptOutcome {
    Rested,
    Fired {
        continuation: String,
        data: Vec<String>,
    },
}

const ACCELERATION_SCRIPT: [ScriptStep; 12] = [
    ScriptStep::Consume {
        channels: &["script-alpha"],
        patterns: &["one"],
        label: "alpha-one",
        persist: false,
    },
    ScriptStep::Consume {
        channels: &["script-alpha", "script-beta"],
        patterns: &["two", "*"],
        label: "join-two",
        persist: false,
    },
    ScriptStep::Consume {
        channels: &["script-beta"],
        patterns: &["*"],
        label: "beta-any",
        persist: true,
    },
    ScriptStep::Produce {
        channel: "script-alpha",
        value: "zero",
    },
    ScriptStep::Produce {
        channel: "script-alpha",
        value: "two",
    },
    ScriptStep::Produce {
        channel: "script-beta",
        value: "b1",
    },
    ScriptStep::Produce {
        channel: "script-alpha",
        value: "one",
    },
    ScriptStep::Consume {
        channels: &["script-alpha"],
        patterns: &["zero"],
        label: "alpha-zero",
        persist: false,
    },
    ScriptStep::Produce {
        channel: "script-beta",
        value: "b2",
    },
    ScriptStep::Consume {
        channels: &["script-alpha"],
        patterns: &["*"],
        label: "alpha-any",
        persist: false,
    },
    ScriptStep::Produce {
        channel: "script-alpha",
        value: "three",
    },
    ScriptStep::Produce {
        channel: "script-beta",
        value: "b3",
    },
];

fn script_pattern(text: &str) -> Pattern {
    match text {
        "*" => Pattern::Wildcard,
        value => Pattern::StringMatch(value.to_owned()),
    }
}

fn script_universe() -> [Vec<String>; 3] {
    [
        vec!["script-alpha".to_owned()],
        vec!["script-alpha".to_owned(), "script-beta".to_owned()],
        vec!["script-beta".to_owned()],
    ]
}

async fn run_script_step(space: &StringSpace, step: &ScriptStep) -> ScriptOutcome {
    match step {
        ScriptStep::Consume {
            channels,
            patterns,
            label,
            persist,
        } => {
            let fired = space
                .consume(
                    channels
                        .iter()
                        .map(|channel| (*channel).to_owned())
                        .collect(),
                    patterns
                        .iter()
                        .map(|pattern| script_pattern(pattern))
                        .collect(),
                    (*label).to_owned(),
                    *persist,
                    BTreeSet::new(),
                )
                .await
                .expect("an in-memory consume succeeds");
            match fired {
                Some((result, matched)) => ScriptOutcome::Fired {
                    continuation: result.continuation,
                    data: matched
                        .into_iter()
                        .map(|binding| binding.matched_datum)
                        .collect(),
                },
                None => ScriptOutcome::Rested,
            }
        }
        ScriptStep::Produce { channel, value } => {
            let fired = space
                .produce((*channel).to_owned(), (*value).to_owned(), false)
                .await
                .expect("an in-memory produce succeeds");
            match fired {
                Some((result, matched, _)) => ScriptOutcome::Fired {
                    continuation: result.continuation,
                    data: matched
                        .into_iter()
                        .map(|binding| binding.matched_datum)
                        .collect(),
                },
                None => ScriptOutcome::Rested,
            }
        }
    }
}

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

#[test]
fn reference_occurrences_are_sound_over_generated_groups() {
    check_property(STORE_CASES, &store_plan(), |plans| {
        let (_history, store) = build_planned_store(&plans);
        let observation = hydrated(&store, &planned_universe(&plans));
        for image in planned_probes(&plans) {
            let expected = expected_occurrences(&plans, &image);
            for occurrence in derive(&observation, &image) {
                assert!(
                    expected.contains(&occurrence),
                    "a derived occurrence is a stored receive at a real position: {occurrence:?}"
                );
            }
        }
        Ok(())
    });
}

#[test]
fn reference_occurrences_are_complete_over_generated_groups() {
    check_property(STORE_CASES, &store_plan(), |plans| {
        let (_history, store) = build_planned_store(&plans);
        let observation = hydrated(&store, &planned_universe(&plans));
        for image in planned_probes(&plans) {
            let derived = derive(&observation, &image);
            for occurrence in expected_occurrences(&plans, &image) {
                assert!(
                    derived.contains(&occurrence),
                    "every stored receive position is derived: {occurrence:?}"
                );
            }
        }
        Ok(())
    });
}

#[test]
fn occurrence_positions_factor_through_the_group() {
    check_property(STORE_CASES, &store_plan(), |plans| {
        let (_history, store) = build_planned_store(&plans);
        let observation = hydrated(&store, &planned_universe(&plans));
        let relation = derive_all(&observation);
        for image in planned_probes(&plans) {
            let occurrences = derive(&observation, &image);
            let mut factored = BTreeSet::new();
            for (group, positions) in channel_groups(&observation, &image) {
                assert_eq!(positions, expected_positions(&group, &image));
                assert_eq!(capacity(&group, &image), positions.len());
                for target in observation.targets(&group) {
                    factored.insert(Occurrence {
                        channel: image.clone(),
                        group: group.clone(),
                        positions: positions.clone(),
                        target,
                    });
                }
            }
            assert_eq!(occurrences, factored, "positions depend only on the group and channel");
            for occurrence in &occurrences {
                assert!(!occurrence.positions.is_empty());
                assert!(
                    occurrences_of(&occurrence.target).contains(occurrence),
                    "the target's own projection contains each of its occurrences"
                );
            }
            assert_eq!(
                relation.get(&image).cloned().unwrap_or_default(),
                occurrences,
                "the bulk derivation equals the per-channel derivation"
            );
        }
        Ok(())
    });
}

#[test]
fn repeated_channels_yield_one_occurrence_with_every_position() {
    let (_history, store) = raw_hot_store();
    let a: &[u8] = &[0x01];
    let b: &[u8] = &[0x00];
    let aba = raw_channels(&[a, b, a]);
    let aab = raw_channels(&[a, a, b]);
    let aba_record = raw_continuation(&aba, "repeated-aba", false, BTreeSet::new());
    let aab_record = raw_continuation(&aab, "repeated-aab", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&aba, aba_record.clone()), Some(true));
    assert_eq!(store.put_continuation(&aab, aab_record.clone()), Some(true));
    let observation = hydrated(&store, &[aba.clone(), aab.clone()]);
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
    assert_eq!(derive(&observation, a), expected_a);
    assert_eq!(derive(&observation, b), expected_b);
    assert_eq!(capacity(&aba_target.group, a), 2);
    assert_eq!(capacity(&aab_target.group, a), 2);
    assert_eq!(capacity(&aab_target.group, b), 1);
    assert!(derive(&observation, ABSENT_IMAGE).is_empty());
}

#[test]
fn prefix_aliased_channel_images_never_share_occurrences() {
    let (_history, store) = raw_hot_store();
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
    let observation = hydrated(&store, &groups);
    for image in IMAGES {
        let occurrences = derive(&observation, image);
        let expected_groups: BTreeSet<GroupKey> = groups
            .iter()
            .map(|channels| raw_key(channels))
            .filter(|key| key.iter().any(|channel| channel.as_slice() == image))
            .collect();
        let derived_groups: BTreeSet<GroupKey> = occurrences
            .iter()
            .map(|occurrence| occurrence.group.clone())
            .collect();
        assert_eq!(
            derived_groups, expected_groups,
            "an image reaches exactly the groups holding it"
        );
        for occurrence in &occurrences {
            assert_eq!(occurrence.channel.as_slice(), image);
            assert_eq!(occurrence.positions, expected_positions(&occurrence.group, image));
        }
    }
    assert_eq!(derive(&observation, &[0x01]).len(), 1);
    assert_eq!(derive(&observation, &[0x01, 0x00]).len(), 1);
    assert_eq!(derive(&observation, &[0x01, 0x00, 0xFF]).len(), 1);
    assert_eq!(derive(&observation, &[]).len(), 1);
    assert_eq!(derive(&observation, &[0x00]).len(), 1);
    assert!(derive(&observation, ABSENT_IMAGE).is_empty());
}

#[test]
fn derivation_ignores_data_and_authority_orders() {
    let (_history, store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![channels.clone()];
    let key = group_key(&channels);
    for body in ["order-first", "order-second", "order-third"] {
        let record = raw_continuation(&channels, body, false, BTreeSet::new());
        assert_eq!(store.put_continuation(&channels, record), Some(true));
    }
    assert_eq!(store.remove_continuation(&channels, 1), Some(()));
    let removed = hydrated(&store, &universe);
    let removed_orders = resting_orders(&removed, &key);
    assert_eq!(removed_orders, BTreeSet::from([1, 3]));
    let data_channel = channels
        .first()
        .cloned()
        .expect("the group has a first channel");
    store.put_datum(&data_channel, Datum::create(&data_channel, "resting-datum".to_owned(), false));
    let with_data = hydrated(&store, &universe);
    assert_eq!(with_data.store, removed.store, "data never moves a continuation version");
    assert_eq!(derive_all(&with_data), derive_all(&removed));
    store.set_state(store.snapshot());
    let renumbered = hydrated(&store, &universe);
    let renumbered_orders = resting_orders(&renumbered, &key);
    assert_eq!(renumbered_orders, BTreeSet::from([1, 2]));
    assert_ne!(renumbered_orders, removed_orders, "set_state renumbers the authority orders");
    assert_eq!(derive_all(&renumbered), derive_all(&removed), "renumbering keeps the relation");
    assert_eq!(renumbered.targets(&key), removed.targets(&key));
}

#[tokio::test(flavor = "current_thread")]
async fn join_bag_projection_matches_derived_groups() {
    let space = string_space(Box::new(PlainMatch)).await;
    let left = "join-left".to_owned();
    let right = "join-right".to_owned();
    let other = "join-other".to_owned();
    let pair = vec![left.clone(), other.clone()];
    let repeated = vec![left.clone(), left.clone(), other.clone()];
    let single = vec![right.clone()];
    let installed = vec![other.clone()];
    rest_receive(
        &space,
        &pair,
        vec![
            Pattern::StringMatch("pair-left".to_owned()),
            Pattern::StringMatch("pair-other".to_owned()),
        ],
        "pair",
        false,
    )
    .await;
    rest_receive(
        &space,
        &repeated,
        vec![
            Pattern::StringMatch("repeated-0".to_owned()),
            Pattern::StringMatch("repeated-1".to_owned()),
            Pattern::StringMatch("repeated-2".to_owned()),
        ],
        "repeated",
        false,
    )
    .await;
    rest_receive(&space, &single, vec![Pattern::Wildcard], "single", false).await;
    let installation = space
        .install(
            installed.clone(),
            vec![Pattern::StringMatch("installed".to_owned())],
            "installed".to_owned(),
        )
        .await
        .expect("an in-memory install succeeds");
    assert_eq!(installation, None);
    let universe = vec![pair, repeated, single, installed];
    let channels = [left, right.clone(), other];
    assert_join_projection(&space, &universe, &channels);
    assert_eq!(produce(&space, &right, "fire").await, Some("single".to_owned()));
    assert_join_projection(&space, &universe, &channels);
    assert!(
        space.get_store().get_joins(&right).is_empty(),
        "a fired single receive leaves no join"
    );
}

#[test]
fn construction_order_does_not_change_the_relation_or_ids() {
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
    assert_construction_order_invariance(&fixed);
    check_property(STORE_CASES, &store_plan(), |plans| {
        assert_construction_order_invariance(&plans);
        Ok(())
    });
}

#[test]
fn receive_ids_resolve_only_to_byte_identical_records() {
    let (_history, store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![channels.clone()];
    let first = raw_continuation(&channels, "resolve-first", false, BTreeSet::new());
    let second = raw_continuation(&channels, "resolve-second", true, BTreeSet::from([1]));
    let installed = raw_continuation(&channels, "resolve-installed", true, BTreeSet::new());
    assert_eq!(store.put_continuation(&channels, first.clone()), Some(true));
    assert_eq!(store.put_continuation(&channels, second.clone()), Some(true));
    assert_eq!(store.install_continuation(&channels, installed.clone()), Some(()));
    let world = Lineage::new(Vec::new()).world(hydrated(&store, &universe));
    let expected = BTreeSet::from([
        expected_target(ReceiveKind::Resting, &channels, &first),
        expected_target(ReceiveKind::Resting, &channels, &second),
        expected_target(ReceiveKind::Installed, &channels, &installed),
    ]);
    let mut resolved = BTreeSet::new();
    for id in all_ids(&world) {
        let target = resolve(&world, &id).expect("a current id resolves");
        assert_eq!(target, id.target, "an id resolves to its own full bytes");
        for perturbed in perturbations(&id.target) {
            let forged = ReceiveId {
                revision: id.revision.clone(),
                target: perturbed,
            };
            assert_eq!(resolve(&world, &forged), None, "a perturbed id never resolves");
        }
        resolved.insert(target);
    }
    assert_eq!(resolved, expected);
}

#[test]
fn metadata_distinct_records_keep_distinct_ids() {
    let (_history, store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![channels.clone()];
    let key = group_key(&channels);
    let base = raw_continuation(&channels, "metadata", false, BTreeSet::new());
    let persistent = raw_continuation(&channels, "metadata", true, BTreeSet::new());
    let peeking = raw_continuation(&channels, "metadata", false, BTreeSet::from([1]));
    let rebodied = raw_continuation(&channels, "metadata-rebodied", false, BTreeSet::new());
    for record in [&base, &persistent, &peeking, &rebodied] {
        assert_eq!(store.put_continuation(&channels, record.clone()), Some(true));
    }
    let observation = hydrated(&store, &universe);
    let targets = observation.targets(&key);
    assert_eq!(targets.len(), 4, "persist, peeks and body each keep a distinct target");
    let identities: BTreeSet<Vec<u8>> = targets
        .iter()
        .map(|target| target.identity.clone())
        .collect();
    assert_eq!(identities.len(), 4, "metadata-distinct records have distinct identities");
    for image in [&[0x01][..], &[0x00][..]] {
        assert_eq!(derive(&observation, image).len(), 4);
    }
    let (_first_history, first_store) = raw_hot_store();
    let (_alias_history, alias_store) = raw_hot_store();
    assert_eq!(first_store.put_continuation(&channels, base.clone()), Some(true));
    assert_eq!(alias_store.put_continuation(&channels, alias_of(&base)), Some(true));
    let first_targets = hydrated(&first_store, &universe).targets(&key);
    let alias_targets = hydrated(&alias_store, &universe).targets(&key);
    let (Some(first_target), Some(alias_target)) =
        (first_targets.iter().next(), alias_targets.iter().next())
    else {
        panic!("each store holds one receive");
    };
    assert_eq!(first_target.identity, alias_target.identity);
    assert_ne!(first_target.source, alias_target.source);
    assert_ne!(first_target, alias_target, "a source alias keeps a distinct id");
}

#[test]
fn colliding_buckets_resolve_by_full_bytes() {
    let (_history, store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01, 0x00], &[0x00]]);
    let universe = vec![channels.clone()];
    for body in ["bucket-first", "bucket-second", "bucket-third"] {
        let record = raw_continuation(&channels, body, false, BTreeSet::new());
        assert_eq!(store.put_continuation(&channels, record), Some(true));
    }
    let installed = raw_continuation(&channels, "bucket-installed", true, BTreeSet::new());
    assert_eq!(store.install_continuation(&channels, installed), Some(()));
    let targets = hydrated(&store, &universe).targets(&group_key(&channels));
    assert_eq!(targets.len(), 4);
    let hashes: [fn(&ReceiveTarget) -> u64; 2] = [constant_hash, identity_prefix_hash];
    for hash in hashes {
        let buckets = BucketedTargets::new(hash, targets.iter().cloned());
        for target in &targets {
            assert_eq!(buckets.bucket_len(target), targets.len(), "every target collides");
            assert_eq!(buckets.resolve(target), Some(target));
            for perturbed in perturbations(target) {
                assert_eq!(
                    buckets.resolve(&perturbed),
                    None,
                    "a colliding id resolves by full bytes"
                );
            }
        }
    }
}

#[test]
fn ids_from_a_prior_revision_or_generation_never_resolve() {
    let history = StubHistory::default();
    let seeded = raw_channels(&[&[0x01]]);
    let historical = raw_channels(&[&[0x00]]);
    let mutated = raw_channels(&[&[0x01, 0x00]]);
    history.put_continuations(historical.clone(), vec![raw_continuation(
        &historical,
        "prior-history",
        false,
        BTreeSet::new(),
    )]);
    let mut state = HotStoreState::<RawChannel, String, String, String>::default();
    state
        .continuations
        .insert(seeded.clone(), vec![raw_continuation(
            &seeded,
            "prior-seeded",
            false,
            BTreeSet::new(),
        )]);
    let store: RawStore =
        HotStoreInstances::create_from_hs_and_hr(state, Box::new(history.clone()));
    let universe = vec![seeded.clone(), historical.clone(), mutated.clone()];
    let mut lineage = Lineage::new(Vec::new());
    let initial = lineage.world(hydrated(&store, &universe));
    let initial_ids = all_ids(&initial);
    assert!(!initial_ids.is_empty());
    for id in &initial_ids {
        assert_eq!(resolve(&initial, id), Some(id.target.clone()));
    }
    let mutation = raw_continuation(&mutated, "prior-mutation", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&mutated, mutation), Some(true));
    let mutated_world = lineage.world(hydrated(&store, &universe));
    for id in &initial_ids {
        assert!(mutated_world.observation.contains_target(&id.target));
        assert_eq!(resolve(&mutated_world, id), None, "a prior-revision id never resolves");
    }
    let mutated_ids = all_ids(&mutated_world);
    store.set_state(store.snapshot());
    lineage.regenerate();
    let replaced = lineage.world(hydrated(&store, &universe));
    for channels in &universe {
        let key = group_key(channels);
        assert_ne!(
            mutated_world.observation.store.0.get(&key),
            replaced.observation.store.0.get(&key),
            "set_state replaces the version of every group, including base-epoch groups"
        );
    }
    for id in mutated_ids.iter().chain(initial_ids.iter()) {
        assert!(replaced.observation.contains_target(&id.target));
        assert_eq!(resolve(&replaced, id), None, "a prior-generation id never resolves");
    }
    assert_eq!(derive_all(&replaced.observation), derive_all(&mutated_world.observation));
}

#[test]
fn first_source_representative_survives_alias_insertion() {
    let (_history, store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![channels.clone()];
    let key = group_key(&channels);
    let first = raw_continuation(&channels, "representative", false, BTreeSet::new());
    let alias = alias_of(&first);
    assert_eq!(store.put_continuation(&channels, first.clone()), Some(true));
    let lineage = Lineage::new(Vec::new());
    let before = lineage.world(hydrated(&store, &universe));
    let mut accelerator = ReferenceAccelerator::new();
    accelerator.rebuild(&before);
    assert_eq!(store.put_continuation(&channels, alias.clone()), Some(false));
    let after = lineage.world(hydrated(&store, &universe));
    assert_eq!(
        before.observation.version(&key),
        after.observation.version(&key),
        "an alias insertion keeps the group version"
    );
    assert_eq!(
        after.observation.targets(&key),
        BTreeSet::from([expected_target(ReceiveKind::Resting, &channels, &first)]),
        "the first source stays the representative"
    );
    accelerator
        .apply(
            &MaintenanceOp::PutResting(expected_target(ReceiveKind::Resting, &channels, &alias)),
            &before,
            &after,
        )
        .expect("an alias insertion is maintainable");
    assert_matches_rebuild(&accelerator, &after);
}

#[test]
fn incremental_insertion_equals_rebuild() {
    let strategy = proptest::collection::vec((0..3usize, record_plan()), 1..=8);
    check_property(STORE_CASES, &strategy, |insertions| {
        let (_history, store) = raw_hot_store();
        let groups = fixed_groups();
        let lineage = Lineage::new(Vec::new());
        let mut maintained = ReferenceAccelerator::new();
        maintained.rebuild(&lineage.world(hydrated(&store, &groups)));
        let mut inserted_count = 0usize;
        for (group_index, plan) in &insertions {
            let channels = groups
                .get(*group_index)
                .expect("a generated group index addresses the fixed groups");
            let continuation = planned_continuation(channels, plan);
            let key = group_key(channels);
            let before = lineage.world(hydrated(&store, &groups));
            let stale = maintained.clone();
            let inserted = store
                .put_continuation(channels, continuation.clone())
                .expect("an in-memory put reports whether it inserted");
            let after = lineage.world(hydrated(&store, &groups));
            match inserted {
                true => {
                    assert_ne!(
                        before.observation.version(&key),
                        after.observation.version(&key),
                        "an insertion replaces the group version"
                    );
                    assert_eq!(stale.classify(&after), AcceleratorClass::Stale);
                    for image in probe_images(&after) {
                        assert_eq!(
                            read(&stale, &after, &after.observation, &image),
                            derive(&after.observation, &image)
                        );
                    }
                    inserted_count = checked_increment(inserted_count);
                }
                false => assert_eq!(after.observation, before.observation),
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
                .expect("a coherent accelerator maintains an insertion");
            assert_matches_rebuild(&maintained, &after);
        }
        assert!(inserted_count > 0, "every generated trace inserts at least once");
        Ok(())
    });
}

#[test]
fn incremental_nonfinal_removal_equals_rebuild() {
    for index in 0..3usize {
        let (_history, store) = raw_hot_store();
        let channels = raw_channels(&[&[0x01], &[0x00], &[0x01]]);
        let universe = vec![channels.clone()];
        let key = group_key(&channels);
        for body in ["removal-first", "removal-second", "removal-third"] {
            let record = raw_continuation(&channels, body, false, BTreeSet::new());
            assert_eq!(store.put_continuation(&channels, record), Some(true));
        }
        let lineage = Lineage::new(Vec::new());
        let before = lineage.world(hydrated(&store, &universe));
        let mut maintained = ReferenceAccelerator::new();
        maintained.rebuild(&before);
        let stale = maintained.clone();
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
        assert_eq!(stale.classify(&after), AcceleratorClass::Stale);
        assert!(!after.observation.contains_target(&named));
        assert_eq!(after.observation.targets(&key).len(), 2);
        maintained
            .apply(&MaintenanceOp::RemoveResting(named), &before, &after)
            .expect("a coherent accelerator maintains a removal");
        assert_matches_rebuild(&maintained, &after);
    }
}

#[test]
fn incremental_final_removal_equals_rebuild_and_clears_the_group() {
    let (_history, store) = raw_hot_store();
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
    let mut maintained = ReferenceAccelerator::new();
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
        .expect("a coherent accelerator maintains a final removal");
    assert_matches_rebuild(&maintained, &after);
    assert!(!after.observation.support().contains(&emptied_key));
    let built = maintained
        .built()
        .expect("a maintained accelerator is built");
    assert!(!built.catalog.value.contains_key(&emptied_key));
    assert!(!built.automaton.value.contains_key(&emptied_key));
    for occurrences in built.occurrences.value.values() {
        for occurrence in occurrences {
            assert_ne!(occurrence.group, emptied_key, "a cleared group leaves no occurrence");
        }
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
    let shared = derive(&after.observation, &[0x01]);
    assert_eq!(shared.len(), 1);
    assert!(
        shared
            .iter()
            .all(|occurrence| occurrence.group == neighbour_key)
    );
}

#[test]
fn install_and_generation_uninstall_equal_rebuild() {
    let (_history, store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![channels.clone()];
    let key = group_key(&channels);
    let twin = raw_continuation(&channels, "install-twin", true, BTreeSet::new());
    assert_eq!(store.put_continuation(&channels, twin.clone()), Some(true));
    let mut lineage = Lineage::new(Vec::new());
    let resting_only = lineage.world(hydrated(&store, &universe));
    let mut maintained = ReferenceAccelerator::new();
    maintained.rebuild(&resting_only);
    assert_eq!(store.install_continuation(&channels, twin.clone()), Some(()));
    lineage.reinstall();
    let installed = lineage.world(hydrated(&store, &universe));
    assert_ne!(
        resting_only.observation.version(&key),
        installed.observation.version(&key),
        "an install replaces the group version"
    );
    assert_eq!(
        installed.observation.targets(&key),
        BTreeSet::from([
            expected_target(ReceiveKind::Resting, &channels, &twin),
            expected_target(ReceiveKind::Installed, &channels, &twin),
        ]),
        "a byte-identical installed twin keeps a distinct target"
    );
    for image in [&[0x01][..], &[0x00][..]] {
        assert_eq!(derive(&installed.observation, image).len(), 2);
    }
    maintained
        .apply(
            &MaintenanceOp::Install(expected_target(ReceiveKind::Installed, &channels, &twin)),
            &resting_only,
            &installed,
        )
        .expect("a coherent accelerator maintains an install");
    assert_matches_rebuild(&maintained, &installed);
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
        .expect("a coherent accelerator maintains a reinstall");
    assert_matches_rebuild(&maintained, &reinstalled);
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
    assert_eq!(maintained.classify(&uninstalled), AcceleratorClass::Foreign);
    for image in probe_images(&uninstalled) {
        assert_eq!(
            maintained.answer(&uninstalled, &image),
            Answer::Fallback(AcceleratorClass::Foreign)
        );
    }
    maintained.rebuild(&uninstalled);
    assert_matches_rebuild(&maintained, &uninstalled);
    assert_eq!(
        uninstalled.observation.targets(&key),
        BTreeSet::from([expected_target(ReceiveKind::Resting, &channels, &twin)])
    );
}

#[test]
fn history_load_and_duplicate_put_are_relation_and_version_neutral() {
    let (history, store) = raw_hot_store();
    let resident = raw_channels(&[&[0x01]]);
    let first_history = raw_channels(&[&[0x00]]);
    let second_history = raw_channels(&[&[0x01, 0x00], &[0x01]]);
    let historical_first = raw_continuation(&first_history, "load-first", false, BTreeSet::new());
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
    let resident_record = raw_continuation(&resident, "load-resident", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&resident, resident_record.clone()), Some(true));
    let universe = vec![resident.clone(), first_history.clone(), second_history.clone()];
    let resident_key = group_key(&resident);
    let lineage = Lineage::new(Vec::new());
    let partial = lineage.world(observe(&*store, &universe, false));
    assert_eq!(
        partial.observation.unloaded,
        BTreeSet::from([group_key(&first_history), group_key(&second_history)])
    );
    let mut maintained = ReferenceAccelerator::new();
    maintained.rebuild(&partial);
    assert_eq!(maintained.classify(&partial), AcceleratorClass::Incomplete);
    let mut before = partial;
    for channels in [&first_history, &second_history] {
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
            .expect("a history load is maintainable");
        before = after;
    }
    assert!(before.observation.is_complete());
    assert_matches_rebuild(&maintained, &before);
    let neutral = hydrated(&store, &universe);
    assert_eq!(neutral, before.observation);
    let duplicates: [(&Vec<RawChannel>, StringContinuation); 3] = [
        (&resident, resident_record.clone()),
        (&resident, alias_of(&resident_record)),
        (&first_history, historical_first.clone()),
    ];
    for (channels, continuation) in duplicates {
        assert_eq!(store.put_continuation(channels, continuation), Some(false));
        assert_eq!(
            hydrated(&store, &universe),
            neutral,
            "a duplicate put keeps every version and target"
        );
    }
}

#[test]
fn clear_and_set_state_invalidate_every_held_version_and_rebuild_equal() {
    let history = StubHistory::default();
    let seeded = raw_channels(&[&[0x01]]);
    let historical = raw_channels(&[&[0x00]]);
    let bumped = raw_channels(&[&[0x01, 0x00]]);
    history.put_continuations(historical.clone(), vec![raw_continuation(
        &historical,
        "generation-history",
        false,
        BTreeSet::new(),
    )]);
    let mut state = HotStoreState::<RawChannel, String, String, String>::default();
    state
        .continuations
        .insert(seeded.clone(), vec![raw_continuation(
            &seeded,
            "generation-seeded",
            false,
            BTreeSet::new(),
        )]);
    let store: RawStore =
        HotStoreInstances::create_from_hs_and_hr(state, Box::new(history.clone()));
    let bumped_record = raw_continuation(&bumped, "generation-bumped", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&bumped, bumped_record), Some(true));
    let universe = vec![seeded.clone(), historical.clone(), bumped.clone()];
    let mut lineage = Lineage::new(Vec::new());
    let original = lineage.world(hydrated(&store, &universe));
    let mut accelerator = ReferenceAccelerator::new();
    accelerator.rebuild(&original);
    let original_snapshot = store.snapshot();
    store.clear();
    lineage.regenerate();
    let cleared = lineage.world(hydrated(&store, &universe));
    assert_every_version_replaced(&original.observation, &cleared.observation, &universe);
    assert_eq!(accelerator.classify(&cleared), AcceleratorClass::Foreign);
    accelerator.rebuild(&cleared);
    assert_matches_rebuild(&accelerator, &cleared);
    store.set_state(original_snapshot);
    lineage.regenerate();
    let restored = lineage.world(hydrated(&store, &universe));
    assert_every_version_replaced(&cleared.observation, &restored.observation, &universe);
    assert_every_version_replaced(&original.observation, &restored.observation, &universe);
    assert_eq!(derive_all(&restored.observation), derive_all(&original.observation));
    assert_eq!(accelerator.classify(&restored), AcceleratorClass::Foreign);
    accelerator.rebuild(&restored);
    assert_matches_rebuild(&accelerator, &restored);
    store.set_state(store.snapshot());
    lineage.regenerate();
    let replaced = lineage.world(hydrated(&store, &universe));
    assert_every_version_replaced(&restored.observation, &replaced.observation, &universe);
    assert_eq!(derive_all(&replaced.observation), derive_all(&restored.observation));
}

#[test]
fn equal_versions_imply_equal_group_relation() {
    let prefix = [
        SpaceStepPlan::Consume {
            group: 0,
            persist: false,
            pattern: 1,
        },
        SpaceStepPlan::Produce {
            channel: 0,
            value: 1,
        },
    ];
    let strategy = proptest::collection::vec(space_step_plan(), 0..=10);
    check_property(TRACE_CASES, &strategy, |suffix| {
        runtime().block_on(async {
            let space = string_space(Box::new(PlainMatch)).await;
            let groups = trace_groups();
            let keys: Vec<GroupKey> = groups.iter().map(|channels| group_key(channels)).collect();
            let capacity = prefix
                .len()
                .checked_add(suffix.len())
                .and_then(|count| count.checked_add(1))
                .expect("the observation count fits usize");
            let mut observations: Vec<Observation> = Vec::with_capacity(capacity);
            observations.push(hydrated_space(&space, &groups));
            for (ordinal, step) in prefix.iter().chain(suffix.iter()).enumerate() {
                apply_space_step(&space, &groups, step, ordinal).await;
                observations.push(hydrated_space(&space, &groups));
            }
            let first_key = keys.first().expect("the trace has a first group");
            let (Some(rested), Some(fired)) = (observations.get(1), observations.get(2)) else {
                panic!("the fixed prefix is observed");
            };
            assert_eq!(rested.targets(first_key).len(), 1, "the prefix receive rests");
            assert!(fired.targets(first_key).is_empty(), "the prefix produce fires it");
            for key in &keys {
                for left in &observations {
                    for right in &observations {
                        match (left.version(key), right.version(key)) {
                            (Some(left_version), Some(right_version))
                                if left_version == right_version =>
                            {
                                assert_eq!(
                                    left.targets(key),
                                    right.targets(key),
                                    "equal group versions imply equal group relations"
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        });
        Ok(())
    });
}

#[test]
fn reference_reads_equal_derivation_in_every_accelerator_state() {
    let (history, store) = raw_hot_store();
    let resident = raw_channels(&[&[0x01]]);
    let historical = raw_channels(&[&[0x01], &[0x00]]);
    history.put_continuations(historical.clone(), vec![raw_continuation(
        &historical,
        "state-history",
        false,
        BTreeSet::new(),
    )]);
    let resident_record = raw_continuation(&resident, "state-resident", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&resident, resident_record), Some(true));
    let universe = vec![resident.clone(), historical.clone()];
    let mut lineage = Lineage::new(Vec::new());
    let partial = lineage.world(observe(&*store, &universe, false));
    let mut incomplete = ReferenceAccelerator::new();
    incomplete.rebuild(&partial);
    let complete = lineage.world(hydrated(&store, &universe));
    let mut nonempty = assert_reads_equal_derivation(
        &incomplete,
        &partial,
        &complete.observation,
        AcceleratorClass::Incomplete,
    );
    let absent = ReferenceAccelerator::new();
    let absent_nonempty = assert_reads_equal_derivation(
        &absent,
        &complete,
        &complete.observation,
        AcceleratorClass::Absent,
    );
    assert!(absent_nonempty > 0, "the absent state is probed where the derivation is nonempty");
    nonempty = nonempty
        .checked_add(absent_nonempty)
        .expect("the probe tally fits usize");
    let mut building = ReferenceAccelerator::new();
    building.begin_build();
    let _ = assert_reads_equal_derivation(
        &building,
        &complete,
        &complete.observation,
        AcceleratorClass::Building,
    );
    let mut coherent = ReferenceAccelerator::new();
    coherent.rebuild(&complete);
    let _ = assert_reads_equal_derivation(
        &coherent,
        &complete,
        &complete.observation,
        AcceleratorClass::Coherent,
    );
    let stale_record = raw_continuation(&resident, "state-stale", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&resident, stale_record), Some(true));
    let stale_world = lineage.world(hydrated(&store, &universe));
    let _ = assert_reads_equal_derivation(
        &coherent,
        &stale_world,
        &stale_world.observation,
        AcceleratorClass::Stale,
    );
    store.set_state(store.snapshot());
    lineage.regenerate();
    let foreign_world = lineage.world(hydrated(&store, &universe));
    let _ = assert_reads_equal_derivation(
        &coherent,
        &foreign_world,
        &foreign_world.observation,
        AcceleratorClass::Foreign,
    );
    assert!(nonempty > 0);
}

#[test]
fn failed_update_leaves_store_and_accelerator_unchanged() {
    let (_history, store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x00]]);
    let universe = vec![channels.clone()];
    let installed = raw_continuation(&channels, "failure-installed", true, BTreeSet::new());
    let resting = raw_continuation(&channels, "failure-resting", false, BTreeSet::new());
    assert_eq!(store.install_continuation(&channels, installed), Some(()));
    assert_eq!(store.put_continuation(&channels, resting), Some(true));
    let refused = hydrated(&store, &universe);
    for index in [0, 2, 7, -1] {
        assert_eq!(store.remove_continuation(&channels, index), None, "index {index} is refused");
        assert_eq!(
            hydrated(&store, &universe),
            refused,
            "a refused removal changes no version or target"
        );
    }
    let lineage = Lineage::new(Vec::new());
    let before = lineage.world(refused);
    let mut accelerator = ReferenceAccelerator::new();
    accelerator.rebuild(&before);
    let inserted = raw_continuation(&channels, "failure-inserted", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&channels, inserted.clone()), Some(true));
    let after = lineage.world(hydrated(&store, &universe));
    let operation =
        MaintenanceOp::PutResting(expected_target(ReceiveKind::Resting, &channels, &inserted));
    for stage in [FaultStage::Catalog, FaultStage::Occurrence, FaultStage::Automaton] {
        let held = accelerator.clone();
        accelerator.arm_fault(stage);
        assert_eq!(accelerator.apply(&operation, &before, &after), Err(Fault::Injected(stage)));
        assert_eq!(accelerator, held, "a failed update publishes nothing");
        assert_eq!(accelerator.classify(&before), AcceleratorClass::Coherent);
        assert_eq!(accelerator.classify(&after), AcceleratorClass::Stale);
        for image in probe_images(&after) {
            assert_eq!(
                accelerator.answer(&after, &image),
                Answer::Fallback(AcceleratorClass::Stale)
            );
            assert_eq!(
                read(&accelerator, &after, &after.observation, &image),
                derive(&after.observation, &image)
            );
        }
    }
    accelerator
        .apply(&operation, &before, &after)
        .expect("an unarmed update is maintained");
    assert_matches_rebuild(&accelerator, &after);
    assert_eq!(
        accelerator.apply(&operation, &before, &after),
        Err(Fault::Unmaintainable(AcceleratorClass::Stale)),
        "an update from a superseded base is refused"
    );
}

#[test]
fn catalog_occurrence_and_automaton_publish_at_one_revision() {
    let (history, store) = raw_hot_store();
    let resident = raw_channels(&[&[0x01], &[0x01, 0x00]]);
    let historical = raw_channels(&[&[0x00]]);
    history.put_continuations(historical.clone(), vec![raw_continuation(
        &historical,
        "publish-history",
        false,
        BTreeSet::new(),
    )]);
    for body in ["publish-first", "publish-second"] {
        let record = raw_continuation(&resident, body, false, BTreeSet::new());
        assert_eq!(store.put_continuation(&resident, record), Some(true));
    }
    let universe = vec![resident.clone(), historical.clone()];
    let resident_key = group_key(&resident);
    let mut lineage = Lineage::new(Vec::new());
    let mut accelerator = ReferenceAccelerator::new();
    let initial = lineage.world(observe(&*store, &universe, false));
    accelerator.rebuild(&initial);
    assert_single_revision(&accelerator, &initial);

    let inserted = raw_continuation(&resident, "publish-third", false, BTreeSet::new());
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
    assert_single_revision(&accelerator, &after);

    let installed = raw_continuation(&resident, "publish-installed", true, BTreeSet::new());
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
    assert_single_revision(&accelerator, &after);

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
    assert_single_revision(&accelerator, &after);

    let before = lineage.world(observe(&*store, &universe, false));
    let loaded = store.get_continuations(&historical);
    let after = lineage.world(observe(&*store, &universe, false));
    let operation = MaintenanceOp::LoadHistory {
        group: group_key(&historical),
        targets: loaded
            .iter()
            .map(|continuation| expected_target(ReceiveKind::Resting, &historical, continuation))
            .collect(),
    };
    accelerator
        .apply(&operation, &before, &after)
        .expect("a history load is maintained");
    assert_single_revision(&accelerator, &after);
    assert!(after.observation.is_complete());
    assert_matches_rebuild(&accelerator, &after);
}

#[test]
fn same_count_group_replacement_never_reuses_a_stale_automaton() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::with_local_recorder(&recorder, || {
        runtime().block_on(async {
            let space = string_space(Box::new(RetainedPathMatch)).await;
            let channel = "replaced-group".to_owned();
            let channels = vec![channel.clone()];
            let universe = vec![channels.clone()];
            let key = group_key(&channels);
            rest_receive(
                &space,
                &channels,
                vec![Pattern::StringMatch("p0".to_owned())],
                "persistent-p0",
                true,
            )
            .await;
            rest_receive(
                &space,
                &channels,
                vec![Pattern::StringMatch("p1".to_owned())],
                "transient-p1",
                false,
            )
            .await;
            assert_eq!(produce(&space, &channel, "zz").await, None);
            let cached = hydrated_space(&space, &universe);
            assert_eq!(produce(&space, &channel, "zz").await, None);
            assert!(
                retained_cache_hits(&snapshotter) > 0,
                "the retained automaton cache is engaged before the replacement"
            );
            assert_eq!(produce(&space, &channel, "p1").await, Some("transient-p1".to_owned()));
            rest_receive(
                &space,
                &channels,
                vec![Pattern::StringMatch("p2".to_owned())],
                "transient-p2",
                false,
            )
            .await;
            assert_eq!(space.get_store().get_continuations(&channels).len(), 2);
            let replaced = hydrated_space(&space, &universe);
            assert_eq!(
                cached.targets(&key).len(),
                replaced.targets(&key).len(),
                "the replacement keeps the candidate count"
            );
            assert_ne!(cached.targets(&key), replaced.targets(&key));
            assert_ne!(cached.version(&key), replaced.version(&key));
            let hits_before_replacement = retained_cache_hits(&snapshotter);
            assert_eq!(
                produce(&space, &channel, "p2").await,
                Some("transient-p2".to_owned()),
                "a datum matching only the replacement fires it"
            );
            assert_eq!(
                retained_cache_hits(&snapshotter),
                hits_before_replacement,
                "a replaced group never reuses the cached automaton"
            );
        });
    });
}

#[tokio::test(flavor = "current_thread")]
async fn stale_accelerator_candidates_never_commit() {
    let space = string_space(Box::new(PlainMatch)).await;
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
    let lineage = Lineage::new(Vec::new());
    let old_world = lineage.world(hydrated_space(&space, &universe));
    let held_ids = mint_ids(&old_world.revision, &derive(&old_world.observation, &image));
    assert!(!held_ids.is_empty());
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
    let current_world = lineage.world(hydrated_space(&space, &universe));
    for id in &held_ids {
        assert!(current_world.observation.contains_target(&id.target));
        assert_eq!(commit_candidate(&current_world, id), CommitOutcome::Stale);
        assert_eq!(commit_candidate(&old_world, id), CommitOutcome::Committed(id.target.clone()));
    }
    let current_ids =
        mint_ids(&current_world.revision, &derive(&current_world.observation, &image));
    assert_eq!(current_ids.len(), 2);
    for id in &current_ids {
        assert_eq!(
            commit_candidate(&current_world, id),
            CommitOutcome::Committed(id.target.clone())
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn enabled_and_disabled_acceleration_select_identically() {
    let enabled_space = string_space(Box::new(RetainedPathMatch)).await;
    let disabled_space = string_space(Box::new(PlainMatch)).await;
    let universe = script_universe();
    let enabled_lineage = Lineage::new(Vec::new());
    let disabled_lineage = Lineage::new(Vec::new());
    let mut fired = 0usize;
    for step in &ACCELERATION_SCRIPT {
        let enabled_outcome = run_script_step(&enabled_space, step).await;
        let disabled_outcome = run_script_step(&disabled_space, step).await;
        assert_eq!(
            enabled_outcome, disabled_outcome,
            "{step:?}: acceleration never changes the selection"
        );
        match enabled_outcome {
            ScriptOutcome::Fired { .. } => fired = checked_increment(fired),
            ScriptOutcome::Rested => {}
        }
        let enabled_world = enabled_lineage.world(hydrated_space(&enabled_space, &universe));
        let disabled_world = disabled_lineage.world(hydrated_space(&disabled_space, &universe));
        assert_eq!(
            derive_all(&enabled_world.observation),
            derive_all(&disabled_world.observation),
            "{step:?}: both spaces derive the same relation"
        );
        let mut accelerated = ReferenceAccelerator::new();
        accelerated.rebuild(&enabled_world);
        let unaccelerated = ReferenceAccelerator::new();
        for image in probe_images(&enabled_world) {
            let accelerated_read =
                read(&accelerated, &enabled_world, &enabled_world.observation, &image);
            let unaccelerated_read =
                read(&unaccelerated, &enabled_world, &enabled_world.observation, &image);
            assert_eq!(accelerated_read, unaccelerated_read);
            assert_eq!(
                mint_ids(&enabled_world.revision, &accelerated_read),
                mint_ids(&enabled_world.revision, &unaccelerated_read),
                "receive identity is accelerator-free"
            );
        }
    }
    assert!(fired > 0, "the script exercises COMMs");
}

#[test]
fn incremental_maintenance_equals_rebuild_over_generated_traces() {
    let strategy = proptest::collection::vec(trace_step_plan(), 1..=12);
    check_property(STORE_CASES, &strategy, |plans| {
        let (history, store) = raw_hot_store();
        let groups = trace_universe();
        let [_, _, historical] = &groups;
        history.put_continuations(historical.clone(), vec![
            raw_continuation(historical, "trace-history-first", false, BTreeSet::new()),
            raw_continuation(historical, "trace-history-second", true, BTreeSet::from([0])),
        ]);
        let steps: Vec<StoreStep<RawChannel, String, String>> =
            plans.iter().map(|plan| store_step(&groups, plan)).collect();
        let report = check_trace(ReferenceAccelerator::new, &*store, &groups, &probes(), &steps);
        assert_eq!(report.steps, steps.len());
        Ok(())
    });
}

#[test]
fn reference_answers_never_include_absent_receives() {
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

type StaleMutation = (&'static str, fn(&RawStore, &[RawChannel]), bool);

#[tokio::test(flavor = "current_thread")]
async fn stale_accelerators_fall_back_after_every_mutation() {
    let mutations: [StaleMutation; 3] = [
        ("insertion", insert_stale_receive, false),
        ("removal", remove_stale_receive, false),
        ("install", install_stale_receive, true),
    ];
    for (name, mutate, reinstalls) in mutations {
        let (_history, store) = raw_hot_store();
        let channels = raw_channels(&[&[0x01], &[0x00]]);
        let universe = vec![channels.clone()];
        for body in ["stale-first", "stale-second"] {
            let record = raw_continuation(&channels, body, false, BTreeSet::new());
            assert_eq!(store.put_continuation(&channels, record), Some(true));
        }
        let mut lineage = Lineage::new(Vec::new());
        let before = lineage.world(hydrated(&store, &universe));
        let mut accelerator = ReferenceAccelerator::new();
        accelerator.rebuild(&before);
        mutate(&store, &channels);
        if reinstalls {
            lineage.reinstall();
        }
        let after = lineage.world(hydrated(&store, &universe));
        assert_stale_fallback(&accelerator, &after, name);
    }
    let space = string_space(Box::new(PlainMatch)).await;
    let channels = vec!["stale-comm".to_owned()];
    let universe = vec![channels.clone()];
    rest_receive(&space, &channels, vec![Pattern::Wildcard], "stale-comm-receive", false).await;
    let lineage = Lineage::new(Vec::new());
    let before = lineage.world(hydrated_space(&space, &universe));
    let mut accelerator = ReferenceAccelerator::new();
    accelerator.rebuild(&before);
    assert_eq!(produce(&space, "stale-comm", "fired").await, Some("stale-comm-receive".to_owned()));
    let after = lineage.world(hydrated_space(&space, &universe));
    assert_stale_fallback(&accelerator, &after, "comm removal");
}

#[test]
fn unloaded_history_groups_make_the_accelerator_incomplete() {
    let (history, store) = raw_hot_store();
    let resident = raw_channels(&[&[0x01]]);
    let historical = raw_channels(&[&[0x01], &[0x00]]);
    history.put_continuations(historical.clone(), vec![
        raw_continuation(&historical, "unloaded-first", false, BTreeSet::new()),
        raw_continuation(&historical, "unloaded-second", true, BTreeSet::new()),
    ]);
    let resident_record = raw_continuation(&resident, "unloaded-resident", false, BTreeSet::new());
    assert_eq!(store.put_continuation(&resident, resident_record), Some(true));
    let universe = vec![resident.clone(), historical.clone()];
    let reads_before = history.continuation_reads();
    let partial = observe(&*store, &universe, false);
    assert_eq!(
        history.continuation_reads(),
        reads_before,
        "a partial observation never reads history"
    );
    assert_eq!(partial.unloaded, BTreeSet::from([group_key(&historical)]));
    assert!(!partial.is_complete());
    let lineage = Lineage::new(Vec::new());
    let partial_world = lineage.world(partial);
    let mut accelerator = ReferenceAccelerator::new();
    accelerator.rebuild(&partial_world);
    assert_eq!(accelerator.classify(&partial_world), AcceleratorClass::Incomplete);
    let complete = hydrated(&store, &universe);
    assert!(history.continuation_reads() > reads_before, "hydration loads the history group");
    let mut missing = 0usize;
    for image in probe_images(&partial_world) {
        let derived = derive(&complete, &image);
        assert_eq!(
            accelerator.answer(&partial_world, &image),
            Answer::Fallback(AcceleratorClass::Incomplete)
        );
        assert_eq!(read(&accelerator, &partial_world, &complete, &image), derived);
        match accelerator.lookup(&image) == derived {
            true => {}
            false => missing = checked_increment(missing),
        }
    }
    assert!(missing > 0, "the unloaded group contributes occurrences the lookup lacks");
}

#[tokio::test(flavor = "current_thread")]
async fn foreign_generation_or_manifest_accelerators_are_never_adopted() {
    let space = string_space(Box::new(PlainMatch)).await;
    let first = vec!["adoption-first".to_owned()];
    let second = vec!["adoption-second".to_owned()];
    let universe = vec![first.clone(), second.clone()];
    rest_receive(&space, &first, vec![Pattern::Wildcard], "first-receive", false).await;
    let first_root = space
        .create_checkpoint()
        .await
        .expect("an in-memory checkpoint succeeds")
        .root;
    let mut lineage = Lineage::new(first_root.bytes());
    let first_world = lineage.world(hydrated_space(&space, &universe));
    let mut held = ReferenceAccelerator::new();
    held.rebuild(&first_world);
    assert_eq!(held.classify(&first_world), AcceleratorClass::Coherent);
    let persisted = held.persist().expect("a built accelerator persists");
    let store = space.get_store();
    store.set_state(store.snapshot());
    lineage.regenerate();
    let regenerated = lineage.world(hydrated_space(&space, &universe));
    assert_eq!(held.classify(&regenerated), AcceleratorClass::Foreign);
    for image in probe_images(&regenerated) {
        assert_eq!(held.answer(&regenerated, &image), Answer::Fallback(AcceleratorClass::Foreign));
        assert_eq!(
            read(&held, &regenerated, &regenerated.observation, &image),
            derive(&regenerated.observation, &image)
        );
    }
    rest_receive(&space, &second, vec![Pattern::Wildcard], "second-receive", false).await;
    let second_root = space
        .create_checkpoint()
        .await
        .expect("an in-memory checkpoint succeeds")
        .root;
    assert_ne!(first_root, second_root);
    lineage.retarget(second_root.bytes());
    let second_world = lineage.world(hydrated_space(&space, &universe));
    let mut candidate = ReferenceAccelerator::new();
    assert_eq!(
        candidate.adopt_or_rebuild(&persisted, &second_world),
        Adoption::Foreign,
        "an accelerator bound to another manifest is never adopted"
    );
    assert_eq!(candidate.classify(&second_world), AcceleratorClass::Coherent);
    let mut distinguishing = 0usize;
    for image in probe_images(&second_world) {
        let derived = derive(&second_world.observation, &image);
        assert_eq!(candidate.answer(&second_world, &image), Answer::Exact(derived.clone()));
        match persisted
            .occurrences
            .get(&image)
            .cloned()
            .unwrap_or_default() ==
            derived
        {
            true => {}
            false => distinguishing = checked_increment(distinguishing),
        }
    }
    assert!(distinguishing > 0, "adopting the foreign accelerator would answer wrongly");
    space
        .reset(&first_root)
        .await
        .expect("an in-memory reset succeeds");
    lineage.retarget(first_root.bytes());
    let reset_world = lineage.world(hydrated_space(&space, &universe));
    let mut adopted = ReferenceAccelerator::new();
    assert_eq!(
        adopted.adopt_or_rebuild(&persisted, &reset_world),
        Adoption::Adopted,
        "an accelerator bound to the current manifest is adopted"
    );
    assert_eq!(adopted.classify(&reset_world), AcceleratorClass::Coherent);
    for image in probe_images(&reset_world) {
        assert_eq!(
            adopted.answer(&reset_world, &image),
            Answer::Exact(derive(&reset_world.observation, &image))
        );
    }
    assert_eq!(
        held.classify(&reset_world),
        AcceleratorClass::Foreign,
        "the held accelerator of an earlier generation stays foreign"
    );
}
